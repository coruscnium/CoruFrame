#[derive(Debug, Clone, PartialEq)]
pub struct Word {
    pub text: String,
    pub left: u32,
    pub top: u32,
    pub width: u32,
    pub height: u32,
    pub confidence: f32,
}

#[derive(Debug, Clone, Default, PartialEq)]
pub struct Ocr {
    pub text: String,
    pub words: Vec<Word>,
}

// Tesseract assigns confidence 0 to some correctly read words on the stylized
// reward cards (Daikyu on a live frame), so only the rows Tesseract marks as
// non-words (-1) are dropped here.
pub const MIN_CONFIDENCE: f32 = 0.0;

// Tesseract TSV: level 5 rows are words, with pixel boxes and a confidence.
// Malformed rows are skipped: one bad row must not throw away the whole frame.
pub fn parse_tsv(source: &str) -> Ocr {
    let mut words = Vec::new();
    for line in source.lines().skip(1) {
        if line.trim().is_empty() {
            continue;
        }
        let fields: Vec<&str> = line.split('\t').collect();
        if fields.len() < 12 || fields[0] != "5" || fields[11].is_empty() {
            continue;
        }
        let Ok(confidence) = fields[10].parse::<f32>() else {
            continue;
        };
        if confidence < MIN_CONFIDENCE {
            continue;
        }
        let numbers: Vec<u32> = [6, 7, 8, 9]
            .iter()
            .map_while(|field| fields[*field].parse::<u32>().ok())
            .collect();
        if numbers.len() != 4 {
            continue;
        }
        words.push(Word {
            text: fields[11].to_string(),
            left: numbers[0],
            top: numbers[1],
            width: numbers[2],
            height: numbers[3],
            confidence,
        });
    }
    let text = words
        .iter()
        .map(|word| word.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    Ocr { text, words }
}

impl Ocr {
    // Mean word confidence, not page confidence: tesseract's page score is
    // dominated by the art around the name. Zero words means no evidence.
    pub fn mean_confidence(&self) -> f32 {
        if self.words.is_empty() {
            return 0.0;
        }
        self.words.iter().map(|word| word.confidence).sum::<f32>() / self.words.len() as f32
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const HEADER: &str =
        "level\tpage_num\tblock_num\tpar_num\tline_num\tword_num\tleft\ttop\twidth\theight\tconf\ttext";

    fn tsv(rows: &[&str]) -> String {
        let mut source = HEADER.to_string();
        for row in rows {
            source.push('\n');
            source.push_str(row);
        }
        source
    }

    #[test]
    fn keeps_low_confidence_words_but_drops_non_words() {
        let ocr = parse_tsv(&tsv(&[
            "5\t1\t1\t1\t1\t1\t10\t20\t30\t40\t91.5\tVOID",
            "5\t1\t1\t1\t1\t2\t50\t20\t60\t40\t80.0\tFISSURE/REWARDS",
            "5\t1\t1\t1\t1\t3\t70\t20\t10\t10\t0.0\tDaikyu",
            "4\t1\t1\t1\t1\t1\t0\t0\t99\t99\t90.0\tline",
            "5\t1\t1\t1\t1\t4\t0\t0\t1\t1\t-1\tignored",
            "5\t1\t1\t1\t1\t5\t0\t0\t1\t1\t0.0\t",
        ]));
        assert_eq!(ocr.text, "VOID FISSURE/REWARDS Daikyu");
        assert_eq!(ocr.words.len(), 3);
        assert_eq!(
            ocr.words[0],
            Word {
                text: "VOID".into(),
                left: 10,
                top: 20,
                width: 30,
                height: 40,
                confidence: 91.5,
            }
        );
    }

    #[test]
    fn skips_a_short_row_instead_of_failing() {
        let ocr = parse_tsv(&tsv(&["5\t1", "5\t1\t1\t1\t1\t1\t0\t0\t10\t10\t90\tCedo"]));
        assert_eq!(ocr.words.len(), 1);
        assert_eq!(ocr.text, "Cedo");
    }

    #[test]
    fn skips_bad_numbers_without_losing_the_frame() {
        let ocr = parse_tsv(&tsv(&[
            "5\t1\t1\t1\t1\t1\tx\t0\t10\t10\t90\tbad",
            "5\t1\t1\t1\t1\t2\t0\t0\t10\t10\t90\tCedo",
        ]));
        assert_eq!(ocr.text, "Cedo");
    }
}
