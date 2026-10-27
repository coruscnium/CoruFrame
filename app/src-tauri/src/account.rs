use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard};

use serde::{Deserialize, Serialize};
use serde_json::{json, Map, Value};

use coruframe_core::catalog::{Catalog, Item};
use coruframe_core::fetch;
use coruframe_core::private_fs;

use crate::market::{self, MarketItemDto};
use crate::wallet::{self, CredentialStore, Storage, StoreErrorKind};

const API: &str = "https://api.warframe.market/v2";
const AUTH_API: &str = "https://api.warframe.market/v1";

#[derive(Clone, Serialize)]
pub struct AccountStatusDto {
    pub connected: bool,
    pub verified: bool,
    pub last_error: Option<String>,
    // "wallet" | "file" | "none" | "unknown"; unknown means "not loaded yet".
    pub storage: String,
    pub needs_unlock: bool,
    pub warning: Option<String>,
}

#[derive(Clone, Serialize)]
pub struct MyOrderDto {
    pub id: String,
    pub item_id: String,
    pub item: Option<MarketItemDto>,
    pub kind: String,
    pub platinum: u64,
    pub quantity: u64,
    pub visible: bool,
    pub per_trade: Option<u64>,
    pub rank: Option<u64>,
    pub charges: Option<u64>,
    pub amber_stars: Option<u64>,
    pub cyan_stars: Option<u64>,
    pub subtype: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

#[derive(Deserialize)]
pub struct OrderInput {
    pub item_id: String,
    pub platinum: u64,
    pub quantity: u64,
    #[serde(default)]
    pub visible: Option<bool>,
    #[serde(default)]
    pub rank: Option<u64>,
    #[serde(default)]
    pub charges: Option<u64>,
    #[serde(default)]
    pub amber_stars: Option<u64>,
    #[serde(default)]
    pub cyan_stars: Option<u64>,
    #[serde(default)]
    pub subtype: Option<String>,
    #[serde(default)]
    pub per_trade: Option<u64>,
}

#[derive(Deserialize)]
struct RawMyOrder {
    id: String,
    #[serde(rename = "type", default)]
    kind: String,
    #[serde(default)]
    platinum: u64,
    #[serde(default)]
    quantity: u64,
    #[serde(default)]
    visible: bool,
    #[serde(default, rename = "itemId")]
    item_id: String,
    #[serde(default)]
    rank: Option<u64>,
    #[serde(default)]
    charges: Option<u64>,
    #[serde(default, rename = "amberStars")]
    amber_stars: Option<u64>,
    #[serde(default, rename = "cyanStars")]
    cyan_stars: Option<u64>,
    #[serde(default)]
    subtype: Option<String>,
    #[serde(default, rename = "perTrade")]
    per_trade: Option<u64>,
    #[serde(default, rename = "createdAt")]
    created_at: String,
    #[serde(default, rename = "updatedAt")]
    updated_at: String,
}

#[derive(Deserialize)]
struct MyOrdersDoc {
    #[serde(default)]
    data: Vec<RawMyOrder>,
}

struct ApiError {
    message: String,
    unverified: bool,
}

impl ApiError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            unverified: false,
        }
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Phase {
    Unloaded,
    Loaded,
    Error,
}

struct Core {
    phase: Phase,
    token: Option<String>,
    verified: bool,
    last_error: Option<String>,
    storage: &'static str,
    needs_unlock: bool,
    warning: Option<String>,
}

impl Core {
    const fn new() -> Self {
        Self {
            phase: Phase::Unloaded,
            token: None,
            verified: false,
            last_error: None,
            storage: "unknown",
            needs_unlock: true,
            warning: None,
        }
    }
}

static CORE: Mutex<Core> = Mutex::new(Core::new());
// Single flight: status, presence and the first market action share one wallet
// read, so a locked wallet cannot produce two unlock prompts.
static LOADING: Mutex<()> = Mutex::new(());

fn core() -> MutexGuard<'static, Core> {
    CORE.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
}

fn dto(core: &Core) -> AccountStatusDto {
    AccountStatusDto {
        connected: core.token.is_some(),
        verified: core.verified,
        last_error: core.last_error.clone(),
        storage: core.storage.to_string(),
        needs_unlock: core.needs_unlock,
        warning: core.warning.clone(),
    }
}

fn disconnected(storage: &'static str, needs_unlock: bool) -> AccountStatusDto {
    AccountStatusDto {
        connected: false,
        verified: false,
        last_error: None,
        storage: storage.to_string(),
        needs_unlock,
        warning: None,
    }
}

fn install_loaded(token: Option<String>, storage: &'static str, warning: Option<String>) {
    let mut core = core();
    core.phase = Phase::Loaded;
    core.token = token;
    core.verified = false;
    core.last_error = None;
    core.storage = storage;
    core.needs_unlock = false;
    core.warning = warning;
}

fn install_error(message: String, storage: &'static str, needs_unlock: bool) {
    let mut core = core();
    core.phase = Phase::Error;
    core.token = None;
    core.verified = false;
    core.last_error = Some(message);
    core.storage = storage;
    core.needs_unlock = needs_unlock;
    core.warning = None;
}

pub fn configured_path() -> PathBuf {
    private_fs::config_dir().join("account-configured")
}

pub fn is_configured() -> bool {
    configured_path().is_file()
}

fn set_configured(on: bool) {
    if on {
        let _ = private_fs::write_private(&configured_path(), b"1");
    } else {
        let _ = std::fs::remove_file(configured_path());
    }
}

fn file_warning(storage: Storage) -> Option<String> {
    (storage == Storage::File).then(|| {
        "The Secret Service wallet is unavailable; the token is stored in an owner-only file."
            .to_string()
    })
}

// v1 sign-in expects one stable id per device, so persist it rather than
// generating a new one for every login.
fn device_id() -> Result<String, String> {
    let path = private_fs::config_dir().join("market-device");
    if let Ok(text) = std::fs::read_to_string(&path) {
        let id = text.trim();
        if !id.is_empty() {
            return Ok(id.to_string());
        }
    }

    let mut bytes = [0u8; 16];
    let random = std::fs::File::open("/dev/urandom")
        .and_then(|mut file| std::io::Read::read_exact(&mut file, &mut bytes))
        .is_ok();
    if !random {
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|elapsed| elapsed.as_nanos())
            .unwrap_or(0) as u64;
        bytes[..8].copy_from_slice(&nanos.to_le_bytes());
        bytes[8..].copy_from_slice(&u64::from(std::process::id()).to_le_bytes());
    }
    // Shape it as a UUIDv4; the API documents UUIDs for device_id.
    bytes[6] = (bytes[6] & 0x0f) | 0x40;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    let hex: String = bytes.iter().map(|byte| format!("{byte:02x}")).collect();
    let id = format!(
        "{}-{}-{}-{}-{}",
        &hex[0..8],
        &hex[8..12],
        &hex[12..16],
        &hex[16..20],
        &hex[20..32]
    );
    private_fs::write_private(&path, id.as_bytes())
        .map_err(|error| format!("{}: {error}", path.display()))?;
    Ok(id)
}

// The token used by presence and riven. Never triggers a second wallet read, and
// never reads the wallet at all for a user with no account.
pub fn load_token() -> Option<String> {
    {
        let core = core();
        match core.phase {
            Phase::Loaded => return core.token.clone(),
            Phase::Error => return None,
            Phase::Unloaded => {}
        }
    }
    if !is_configured() && !wallet::legacy_token_exists() {
        return None;
    }
    ensure_loaded(false);
    core().token.clone()
}

// The non-secret snapshot. `force` is the manual Refresh path, which bypasses the
// account-configured marker; the automatic startup path never does.
pub fn status(force: bool) -> AccountStatusDto {
    if force {
        return ensure_loaded(true);
    }
    let core = core();
    if core.phase != Phase::Unloaded {
        return dto(&core);
    }
    if !is_configured() && !wallet::legacy_token_exists() {
        return disconnected("none", false);
    }
    // Do not touch the secret from a status poll; the background load owns it.
    disconnected("unknown", true)
}

enum LoadOutcome {
    Token { token: String, migrated: bool },
    Empty,
    Error { message: String, needs_unlock: bool },
}

fn load_outcome(store: &dyn CredentialStore, legacy: Option<String>) -> LoadOutcome {
    match store.load() {
        Ok(Some(raw)) => match wallet::validate(&raw) {
            Ok(token) => LoadOutcome::Token {
                token,
                migrated: false,
            },
            Err(message) => LoadOutcome::Error {
                message: format!("{message}; sign in again"),
                needs_unlock: false,
            },
        },
        Err(error) if error.kind == StoreErrorKind::Failed => LoadOutcome::Error {
            message: error.message,
            needs_unlock: true,
        },
        Err(error) if error.kind == StoreErrorKind::Unavailable => LoadOutcome::Error {
            message: error.message,
            needs_unlock: false,
        },
        Ok(None) | Err(_) => match legacy {
            None => LoadOutcome::Empty,
            Some(raw) => match wallet::validate(&raw) {
                Err(message) => LoadOutcome::Error {
                    message: format!("{message}; the token file was kept, sign in again"),
                    needs_unlock: false,
                },
                Ok(token) => match store.save(&token) {
                    Err(error) => LoadOutcome::Error {
                        message: format!("could not migrate the token: {}", error.message),
                        needs_unlock: error.kind == StoreErrorKind::Failed,
                    },
                    Ok(()) => match store.load() {
                        Ok(Some(readback)) if wallet::normalize(&readback) == token => {
                            LoadOutcome::Token {
                                token,
                                migrated: true,
                            }
                        }
                        // The file stays: a failed read-back must not lose the
                        // only copy of the credential.
                        _ => LoadOutcome::Error {
                            message:
                                "the wallet did not return the migrated token; the token file was kept"
                                    .into(),
                            needs_unlock: false,
                        },
                    },
                },
            },
        },
    }
}

// Loads the credential once. A missing marker and no legacy file short-circuit
// before the wallet so a fresh install never sees an unlock prompt.
pub fn ensure_loaded(force: bool) -> AccountStatusDto {
    if !force {
        let core = core();
        if core.phase != Phase::Unloaded {
            return dto(&core);
        }
    }
    let _loading = LOADING
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner());
    if !force {
        let core = core();
        if core.phase != Phase::Unloaded {
            return dto(&core);
        }
    }

    if !is_configured() && !wallet::legacy_token_exists() {
        install_loaded(None, "none", None);
        return dto(&core());
    }

    // The legacy token must be 0600 before it is read, even on an install that
    // an older build left with default modes.
    for skipped in private_fs::harden_existing_paths() {
        eprintln!("permissions: {skipped}");
    }
    let (store, storage) = wallet::choose(wallet::force_file_store());
    let legacy = std::fs::read_to_string(wallet::legacy_token_path()).ok();
    match load_outcome(store.as_ref(), legacy) {
        LoadOutcome::Token { token, migrated } => {
            if migrated && storage == Storage::Wallet {
                let _ = std::fs::remove_file(wallet::legacy_token_path());
                set_configured(true);
            }
            install_loaded(Some(token), storage.name(), file_warning(storage));
        }
        LoadOutcome::Empty => {
            install_loaded(None, storage.name(), file_warning(storage));
        }
        LoadOutcome::Error {
            message,
            needs_unlock,
        } => install_error(message, storage.name(), needs_unlock),
    }
    dto(&core())
}

// Strictly alphanumeric: the id is interpolated into a URL path.
fn valid_order_id(id: &str) -> bool {
    !id.is_empty() && id.len() <= 64 && id.bytes().all(|byte| byte.is_ascii_alphanumeric())
}

fn status_now() -> AccountStatusDto {
    dto(&core())
}

fn fail(error: ApiError) -> String {
    let mut core = core();
    if error.unverified {
        core.verified = false;
    }
    core.last_error = Some(error.message.clone());
    error.message
}

// Maps both the legacy token (401) and the banned/unverified account (403) to a
// sentence the user can act on, and field errors to their input name.
pub fn error_message(status: u16, body: &str) -> (String, bool) {
    let doc: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let request = doc
        .get("error")
        .and_then(|error| error.get("request"))
        .and_then(Value::as_array)
        .map(|codes| {
            codes
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();

    let unauthorized =
        status == 401 || request.iter().any(|code| code == "app.errors.unauthorized");
    if unauthorized {
        return (
            "warframe.market rejected the token. Sign in again.".to_string(),
            true,
        );
    }
    if status == 403 || request.iter().any(|code| code == "app.errors.forbidden") {
        return (
            "This account cannot write orders (unverified or banned by warframe.market)."
                .to_string(),
            false,
        );
    }
    if status == 429 || status == 509 || request.iter().any(|code| code.contains("rateLimit")) {
        return (
            "warframe.market is rate limiting; try again in a few seconds.".to_string(),
            false,
        );
    }

    if let Some(inputs) = doc
        .get("error")
        .and_then(|error| error.get("inputs"))
        .and_then(Value::as_object)
    {
        let messages: Vec<String> = inputs
            .iter()
            .filter_map(|(field, code)| {
                let code = code.as_str()?.rsplit('.').next()?.replace('_', " ");
                Some(format!("{field}: {code}"))
            })
            .collect();
        if !messages.is_empty() {
            return (messages.join("; "), false);
        }
    }

    let fallback = request
        .first()
        .cloned()
        .unwrap_or_else(|| format!("HTTP {status}"));
    (
        format!(
            "{fallback}: {}",
            body.trim().chars().take(200).collect::<String>()
        ),
        false,
    )
}

fn call(method: &str, path: &str, token: &str, body: Option<&Value>) -> Result<Value, ApiError> {
    let encoded = body.map(|value| value.to_string());
    let response = fetch::request(
        method,
        &format!("{API}{path}"),
        Some(token),
        encoded.as_deref(),
    )
    .map_err(ApiError::new)?;
    if !(200..300).contains(&response.status) {
        let (message, unverified) = error_message(response.status, &response.body);
        return Err(ApiError {
            message,
            unverified,
        });
    }
    if response.body.trim().is_empty() {
        return Ok(Value::Null);
    }
    serde_json::from_str(&response.body)
        .map_err(|error| ApiError::new(format!("market json: {error}")))
}

fn token_of() -> Result<String, ApiError> {
    load_token()
        .ok_or_else(|| ApiError::new("Connect a warframe.market account in Settings first."))
}

fn mark_ok() {
    let mut core = core();
    core.verified = true;
    core.last_error = None;
}

// The token is written to the chosen store before the in-memory state changes,
// so a save failure leaves the UI disconnected instead of pretending.
fn persist(token: &str) -> Result<Storage, String> {
    let (store, storage) = wallet::choose(wallet::force_file_store());
    store
        .save(token)
        .map_err(|error| format!("could not save the token: {}", error.message))?;
    set_configured(true);
    Ok(storage)
}

fn install_token(token: String, storage: Storage, verified: bool) {
    let mut core = core();
    core.phase = Phase::Loaded;
    core.token = Some(token);
    core.verified = verified;
    core.last_error = None;
    core.storage = storage.name();
    core.needs_unlock = false;
    core.warning = file_warning(storage);
}

pub fn connect(token: &str) -> Result<AccountStatusDto, String> {
    let token = wallet::validate(token).inspect_err(|message| {
        core().last_error = Some(message.clone());
    })?;
    call("GET", "/orders/my", &token, None).map_err(|error| {
        core().last_error = Some(error.message.clone());
        error.message
    })?;
    let storage = persist(&token)?;
    install_token(token, storage, true);
    Ok(status_now())
}

// Password login through the legacy v1 endpoint. The password only exists in
// this call and on curl's stdin; only the token is persisted.
pub fn login(email: &str, password: &str) -> Result<AccountStatusDto, String> {
    let email = email.trim();
    if email.is_empty() {
        return Err("Enter your warframe.market email or username.".into());
    }
    if password.is_empty() {
        return Err("Enter your warframe.market password.".into());
    }

    let body = json!({
        "auth_type": "header",
        "email": email,
        "password": password,
        "device_id": device_id()?,
    });
    let response = fetch::request_with_header(
        "POST",
        &format!("{AUTH_API}/auth/signin"),
        Some("JWT"),
        Some(&body.to_string()),
    )
    .map_err(|error| {
        let message = format!("warframe.market login: {error}");
        core().last_error = Some(message.clone());
        message
    })?;
    if response.status != 200 {
        let message = signin_error(response.status, &response.body);
        core().last_error = Some(message.clone());
        return Err(message);
    }

    let token = response
        .headers
        .iter()
        .find(|(name, _)| name == "authorization")
        .map(|(_, value)| wallet::normalize(value))
        .filter(|value| !value.is_empty())
        .ok_or("warframe.market accepted the login but returned no token.")?;
    let storage = persist(&token)?;
    install_token(token.clone(), storage, false);

    // A valid login can still be read-only (unverified account): keep the
    // token and surface the restriction instead of pretending login failed.
    match call("GET", "/orders/my", &token, None) {
        Ok(_) => {
            mark_ok();
            Ok(status_now())
        }
        Err(error) if error.unverified => {
            let _ = (wallet::choose(wallet::force_file_store()).0).delete();
            let _ = std::fs::remove_file(wallet::legacy_token_path());
            set_configured(false);
            let mut core = core();
            core.phase = Phase::Loaded;
            core.token = None;
            core.storage = "none";
            core.last_error = Some(error.message.clone());
            Err(error.message)
        }
        Err(error) => {
            core().last_error = Some(error.message.clone());
            Ok(status_now())
        }
    }
}

fn signin_error(status: u16, body: &str) -> String {
    let doc: Value = serde_json::from_str(body).unwrap_or(Value::Null);
    let error = doc.get("error");
    let field = |name: &str| -> Vec<String> {
        match error.and_then(|error| error.get(name)) {
            Some(Value::Array(values)) => values
                .iter()
                .filter_map(Value::as_str)
                .map(ToString::to_string)
                .collect(),
            Some(Value::String(value)) => vec![value.clone()],
            _ => Vec::new(),
        }
    };
    let codes = [field("email"), field("password"), field("request")].concat();
    let joined = codes.join(" ");
    if joined.contains("email_not_exist") {
        return "No warframe.market account uses that email or username.".into();
    }
    if joined.contains("wrong_password") {
        return "Wrong password.".into();
    }
    if joined.contains("not_verified") {
        return "Verify your email on warframe.market before signing in.".into();
    }
    if joined.contains("banned") {
        return "This account is banned from warframe.market.".into();
    }
    if status == 429 || joined.contains("rateLimit") {
        return "warframe.market is rate limiting logins; wait a minute and retry.".into();
    }
    error_message(status, body).0
}

pub fn disconnect() -> AccountStatusDto {
    let (store, _) = wallet::choose(wallet::force_file_store());
    let wallet_error = store.delete().err();
    // Remove the legacy file too, so a rollback build cannot resurrect the token.
    let _ = std::fs::remove_file(wallet::legacy_token_path());
    set_configured(false);
    let mut core = core();
    core.phase = Phase::Loaded;
    core.token = None;
    core.verified = false;
    core.storage = "none";
    core.warning = None;
    if let Some(error) = wallet_error {
        core.last_error = Some(format!(
            "the wallet entry could not be deleted: {}",
            error.message
        ));
        core.needs_unlock = error.kind == StoreErrorKind::Failed;
    } else {
        core.last_error = None;
        core.needs_unlock = false;
    }
    dto(&core)
}

pub fn my_orders(catalog: &Catalog) -> Result<Vec<MyOrderDto>, String> {
    let token = token_of().map_err(fail)?;
    let doc = call("GET", "/orders/my", &token, None).map_err(fail)?;
    mark_ok();
    let doc: MyOrdersDoc =
        serde_json::from_value(doc).map_err(|error| format!("market orders json: {error}"))?;
    Ok(doc
        .data
        .into_iter()
        .map(|order| MyOrderDto {
            item: catalog.by_id(&order.item_id).map(market::item_dto),
            item_id: order.item_id,
            id: order.id,
            kind: order.kind,
            platinum: order.platinum,
            quantity: order.quantity,
            visible: order.visible,
            per_trade: order.per_trade,
            rank: order.rank,
            charges: order.charges,
            amber_stars: order.amber_stars,
            cyan_stars: order.cyan_stars,
            subtype: order.subtype,
            created_at: order.created_at,
            updated_at: order.updated_at,
        })
        .collect())
}

// Builds the write body from the item's caps so the API never receives a field
// it rejects, and perTrade only for bulk-tradable items.
pub fn payload(item: &Item, input: &OrderInput) -> Result<Value, String> {
    if item.id.is_empty() {
        return Err("this item has no market id".into());
    }
    if !(1..=900_000).contains(&input.platinum) {
        return Err("platinum must be between 1 and 900000".into());
    }
    if !(1..=9_999).contains(&input.quantity) {
        return Err("quantity must be between 1 and 9999".into());
    }

    let mut map = Map::new();
    map.insert("itemId".into(), json!(item.id));
    map.insert("type".into(), json!("sell"));
    map.insert("platinum".into(), json!(input.platinum));
    map.insert("quantity".into(), json!(input.quantity));
    map.insert("visible".into(), json!(input.visible.unwrap_or(true)));

    if let Some(max) = item.max_rank {
        // Never silently list at max rank: the user may only own a lower copy.
        let rank = input
            .rank
            .ok_or_else(|| "rank is required for this item".to_string())?;
        if rank > max {
            return Err(format!("rank must be at most {max}"));
        }
        map.insert("rank".into(), json!(rank));
    }
    if let Some(max) = item.max_charges {
        let charges = input.charges.unwrap_or(max);
        if charges > max {
            return Err(format!("charges must be at most {max}"));
        }
        map.insert("charges".into(), json!(charges));
    }
    if let Some(max) = item.max_amber_stars {
        let stars = input.amber_stars.unwrap_or(max);
        if stars > max {
            return Err(format!("amber stars must be at most {max}"));
        }
        map.insert("amberStars".into(), json!(stars));
    }
    if let Some(max) = item.max_cyan_stars {
        let stars = input.cyan_stars.unwrap_or(max);
        if stars > max {
            return Err(format!("cyan stars must be at most {max}"));
        }
        map.insert("cyanStars".into(), json!(stars));
    }
    if let Some(subtypes) = item.subtypes.as_ref() {
        let subtype = input.subtype.clone().or_else(|| subtypes.first().cloned());
        if let Some(subtype) = subtype {
            if !subtypes.contains(&subtype) {
                return Err(format!("unknown subtype {subtype}"));
            }
            map.insert("subtype".into(), json!(subtype));
        }
    }
    let per_trade = input.per_trade.unwrap_or(1);
    if per_trade > 1 {
        if !(1..=6).contains(&per_trade) || !input.quantity.is_multiple_of(per_trade) {
            return Err("per-trade must be 1..6 and divide the quantity".into());
        }
        map.insert("perTrade".into(), json!(per_trade));
    }
    Ok(Value::Object(map))
}

fn payload_for_update(item: &Item, input: &OrderInput) -> Result<Value, String> {
    let mut value = payload(item, input)?;
    if let Some(map) = value.as_object_mut() {
        map.remove("itemId");
        map.remove("type");
    }
    Ok(value)
}

fn item_of<'a>(catalog: &'a Catalog, input: &OrderInput) -> Result<&'a Item, String> {
    catalog
        .by_id(&input.item_id)
        .or_else(|| catalog.resolve(&input.item_id))
        .ok_or_else(|| format!("unknown market item: {}", input.item_id))
}

pub fn create(catalog: &Catalog, input: &OrderInput) -> Result<Option<String>, String> {
    let item = item_of(catalog, input)?;
    let body = payload(item, input)?;
    let token = token_of().map_err(fail)?;
    let created = call("POST", "/order", &token, Some(&body)).map_err(fail)?;
    mark_ok();
    Ok(created
        .get("data")
        .and_then(|data| data.get("id"))
        .and_then(Value::as_str)
        .map(ToString::to_string))
}

pub fn update(catalog: &Catalog, id: &str, input: &OrderInput) -> Result<(), String> {
    if !valid_order_id(id) {
        return Err("invalid order id".into());
    }
    let item = item_of(catalog, input)?;
    let body = payload_for_update(item, input)?;
    let token = token_of().map_err(fail)?;
    call("PATCH", &format!("/order/{id}"), &token, Some(&body)).map_err(fail)?;
    mark_ok();
    Ok(())
}

pub fn close(id: &str, quantity: u64) -> Result<(), String> {
    if !valid_order_id(id) {
        return Err("invalid order id".into());
    }
    if !(1..=9_999).contains(&quantity) {
        return Err("quantity must be between 1 and 9999".into());
    }
    let token = token_of().map_err(fail)?;
    let body = json!({ "quantity": quantity });
    call("POST", &format!("/order/{id}/close"), &token, Some(&body)).map_err(fail)?;
    mark_ok();
    Ok(())
}

pub fn delete(id: &str) -> Result<(), String> {
    if !valid_order_id(id) {
        return Err("invalid order id".into());
    }
    let token = token_of().map_err(fail)?;
    call("DELETE", &format!("/order/{id}"), &token, None).map_err(fail)?;
    mark_ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::wallet::{CredentialStore, StoreError};

    fn item() -> Item {
        let catalog = Catalog::from_json(
            r#"{"data":[
              {"id":"mod1","slug":"vitality","gameRef":"/Vitality","tags":["mod"],
               "maxRank":10,"subtypes":["regular"],"i18n":{"en":{"name":"Vitality"}}},
              {"id":"arcane1","slug":"arcane_energize","gameRef":"/Arcane","tags":["arcane_enhancement"],
               "bulkTradable":true,"maxRank":5,"i18n":{"en":{"name":"Arcane Energize"}}},
              {"id":"frame1","slug":"excalibur","gameRef":"/Excalibur","tags":[],
               "i18n":{"en":{"name":"Excalibur"}}}
            ]}"#,
        )
        .unwrap();
        catalog.resolve("vitality").unwrap().clone()
    }

    fn input(item_id: &str) -> OrderInput {
        OrderInput {
            item_id: item_id.into(),
            platinum: 20,
            quantity: 4,
            visible: Some(true),
            rank: None,
            charges: None,
            amber_stars: None,
            cyan_stars: None,
            subtype: None,
            per_trade: None,
        }
    }

    struct FakeStore {
        value: Mutex<Option<String>>,
        ignore_save: bool,
        lock: bool,
    }

    impl FakeStore {
        fn empty() -> Self {
            Self {
                value: Mutex::new(None),
                ignore_save: false,
                lock: false,
            }
        }
        fn locked() -> Self {
            Self {
                lock: true,
                ..Self::empty()
            }
        }
    }

    impl CredentialStore for FakeStore {
        fn load(&self) -> Result<Option<String>, StoreError> {
            if self.lock {
                return Err(StoreError {
                    kind: StoreErrorKind::Failed,
                    message: "wallet is locked".into(),
                });
            }
            Ok(self.value.lock().unwrap().clone())
        }

        fn save(&self, token: &str) -> Result<(), StoreError> {
            if !self.ignore_save {
                *self.value.lock().unwrap() = Some(token.into());
            }
            Ok(())
        }

        fn delete(&self) -> Result<(), StoreError> {
            *self.value.lock().unwrap() = None;
            Ok(())
        }
    }

    const TOKEN: &str = "eyJhbGciOiJIUzI1NiJ9.eyJzdWIiOiIxMjM0NTY3ODkwIn0.abc123";

    #[test]
    fn migrates_a_legacy_token_after_a_verified_read_back() {
        let store = FakeStore::empty();
        match load_outcome(&store, Some(format!("JWT {TOKEN}"))) {
            LoadOutcome::Token { token, migrated } => {
                assert_eq!(token, TOKEN);
                assert!(migrated);
            }
            _ => panic!("expected a migration"),
        }
        assert_eq!(store.value.lock().unwrap().as_deref(), Some(TOKEN));
    }

    #[test]
    fn a_failed_read_back_keeps_the_file_and_reports_an_error() {
        let store = FakeStore {
            ignore_save: true,
            ..FakeStore::empty()
        };
        assert!(matches!(
            load_outcome(&store, Some(TOKEN.into())),
            LoadOutcome::Error { .. }
        ));
    }

    #[test]
    fn a_locked_wallet_never_falls_back_to_the_file() {
        let outcome = load_outcome(&FakeStore::locked(), Some(TOKEN.into()));
        match outcome {
            LoadOutcome::Error { needs_unlock, .. } => assert!(needs_unlock),
            _ => panic!("a locked wallet must be an error"),
        }
    }

    #[test]
    fn an_invalid_legacy_token_is_kept() {
        let store = FakeStore::empty();
        match load_outcome(&store, Some("too short".into())) {
            LoadOutcome::Error { message, .. } => assert!(message.contains("kept")),
            _ => panic!("an invalid token must not migrate"),
        }
        assert!(store.value.lock().unwrap().is_none());
    }

    #[test]
    fn no_legacy_token_and_an_empty_wallet_loads_empty() {
        assert!(matches!(
            load_outcome(&FakeStore::empty(), None),
            LoadOutcome::Empty
        ));
    }

    #[test]
    fn order_ids_are_strictly_alphanumeric() {
        assert!(valid_order_id("64f0c1a2b3"));
        assert!(!valid_order_id(""));
        assert!(!valid_order_id("../orders"));
        assert!(!valid_order_id("id?x=1"));
        assert!(!valid_order_id("id/close"));
        assert!(!valid_order_id("id with space"));
    }

    #[test]
    fn payload_requires_the_owned_rank() {
        let item = item();
        let mut order = input(&item.id);
        order.quantity = 5;
        let error = payload(&item, &order).unwrap_err();
        assert!(error.contains("rank"), "{error}");

        order.rank = Some(7);
        let body = payload(&item, &order).unwrap();
        assert_eq!(body["rank"], 7);
        assert_eq!(body["subtype"], "regular");
        assert_eq!(body["type"], "sell");
        assert_eq!(body["quantity"], 5);
        assert!(body.get("perTrade").is_none());

        let catalog = Catalog::from_json(
            r#"{"data":[{"id":"arcane1","slug":"arcane_energize","gameRef":"/Arcane",
               "tags":["arcane_enhancement"],"bulkTradable":true,"maxRank":5,
               "i18n":{"en":{"name":"Arcane Energize"}}}]}"#,
        )
        .unwrap();
        let arcane = catalog.resolve("arcane_energize").unwrap();
        let mut order = input(&arcane.id);
        order.rank = Some(0);
        order.per_trade = Some(3);
        assert!(payload(arcane, &order).is_err(), "3 does not divide 4");
        order.per_trade = Some(2);
        let body = payload(arcane, &order).unwrap();
        assert_eq!(body["perTrade"], 2);
        assert_eq!(body["rank"], 0);
    }

    #[test]
    fn payload_rejects_out_of_range_values() {
        let item = item();
        let mut order = input(&item.id);
        order.rank = Some(5);
        order.platinum = 0;
        assert!(payload(&item, &order).is_err());
        order.platinum = 20;
        order.quantity = 10_000;
        assert!(payload(&item, &order).is_err());
        order.quantity = 1;
        order.rank = Some(11);
        assert!(payload(&item, &order).is_err());
    }

    #[test]
    fn parses_unauthorized_and_field_errors() {
        let (message, unverified) =
            error_message(401, r#"{"error":{"request":["app.errors.unauthorized"]}}"#);
        assert!(unverified);
        assert!(message.contains("rejected"));
        let (message, unverified) = error_message(
            400,
            r#"{"error":{"request":[],"inputs":{"platinum":"app.field.tooSmall"}}}"#,
        );
        assert!(!unverified);
        assert!(message.contains("platinum"));
        assert!(message.contains("tooSmall") || message.contains("too small"));
    }

    #[test]
    fn signin_errors_are_actionable() {
        assert_eq!(
            signin_error(
                400,
                r#"{"error":{"email":["app.account.email_not_exist"]}}"#
            ),
            "No warframe.market account uses that email or username."
        );
        assert_eq!(
            signin_error(
                400,
                r#"{"error":{"password":["app.account.wrong_password"]}}"#
            ),
            "Wrong password."
        );
        assert!(
            signin_error(400, r#"{"error":{"password":["app.account.user_banned"]}}"#)
                .contains("banned")
        );
    }
}
