use coruframe_core::overlay_config::Monitor;
use gtk4::prelude::*;

type Result<T> = std::result::Result<T, Box<dyn std::error::Error>>;

// GDK4 calls the first monitor the primary one; there is no separate flag.
pub fn run(args: &[String]) -> Result<()> {
    match args {
        [] => {}
        [flag] if flag == "--json" => {}
        [other, ..] => return Err(format!("unknown option: {other}").into()),
    }
    gtk4::init()?;
    let display = gtk4::gdk::Display::default().ok_or("no Wayland display")?;
    let model = display.monitors();
    let mut monitors = Vec::new();
    for index in 0..model.n_items() {
        let Some(monitor) = model.item(index).and_downcast::<gtk4::gdk::Monitor>() else {
            continue;
        };
        let geometry = monitor.geometry();
        monitors.push(Monitor {
            connector: monitor
                .connector()
                .map(|name| name.to_string())
                .unwrap_or_else(|| format!("monitor-{index}")),
            model: monitor
                .model()
                .map(|name| name.to_string())
                .unwrap_or_else(|| "monitor".to_string()),
            primary: index == 0,
            width: geometry.width(),
            height: geometry.height(),
            scale: monitor.scale_factor(),
        });
    }
    println!("{}", serde_json::to_string_pretty(&monitors)?);
    Ok(())
}
