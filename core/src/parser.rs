use crate::ids;
use crate::model::{Block, Workout};
use serde::Serialize;

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct ParseError {
    pub line: usize,
    pub message: String,
}

fn err(line: usize, message: impl Into<String>) -> ParseError {
    ParseError {
        line,
        message: message.into(),
    }
}

/// Format seconds as `M:SS` (the canonical form `parse_duration` accepts).
fn fmt_secs(secs: u32) -> String {
    format!("{}:{:02}", secs / 60, secs % 60)
}

/// Serialize a workout back to the markdown format `parse_workout` reads.
/// For any workout that came out of the parser, parsing the result yields an
/// equal workout (round-trip).
pub fn workout_to_markdown(w: &Workout) -> String {
    let mut out = format!("# {}\n", w.name);
    if let Some(id) = &w.id {
        out.push_str(&format!("- id: {id}\n"));
    }
    for b in &w.blocks {
        out.push_str(&format!("\n## {}\n", b.name));
        out.push_str(&format!("- intervals: {}\n", b.intervals));
        out.push_str(&format!("- work: {}\n", fmt_secs(b.work_secs)));
        if let Some(r) = b.rest_secs {
            out.push_str(&format!("- rest: {}\n", fmt_secs(r)));
        }
        if let Some(r) = b.rest_after_secs {
            out.push_str(&format!("- rest after: {}\n", fmt_secs(r)));
        }
        if let Some(c) = &b.color {
            out.push_str(&format!("- color: {c}\n"));
        }
        if !b.description_md.is_empty() {
            out.push('\n');
            out.push_str(&b.description_md);
            out.push('\n');
        }
    }
    out
}

/// Render a markdown fragment to HTML (used for block descriptions).
pub fn render_markdown(md: &str) -> String {
    let mut out = String::new();
    pulldown_cmark::html::push_html(&mut out, pulldown_cmark::Parser::new(md));
    out
}

/// Parse a duration written as plain seconds ("90"), "M:SS" ("1:30") or "H:MM:SS".
pub fn parse_duration(val: &str) -> Result<u32, String> {
    let parts: Vec<&str> = val.split(':').collect();
    if parts.len() > 3 || parts.iter().any(|p| p.is_empty() || !p.chars().all(|c| c.is_ascii_digit())) {
        return Err(format!("invalid time '{val}' — use seconds (90) or M:SS (1:30)"));
    }
    let nums: Vec<u32> = parts
        .iter()
        .map(|p| p.parse::<u32>().map_err(|_| format!("invalid time '{val}'")))
        .collect::<Result<_, _>>()?;
    let secs = match nums.as_slice() {
        [s] => *s,
        [m, s] if *s < 60 => m * 60 + s,
        [h, m, s] if *m < 60 && *s < 60 => h * 3600 + m * 60 + s,
        _ => return Err(format!("invalid time '{val}' — minutes/seconds must be < 60")),
    };
    Ok(secs)
}

fn valid_color(val: &str) -> bool {
    val.starts_with('#')
        && matches!(val.len(), 4 | 7)
        && val[1..].chars().all(|c| c.is_ascii_hexdigit())
}

#[derive(Default)]
struct BlockBuilder {
    name: String,
    line: usize,
    description: String,
    intervals: Option<u32>,
    work_secs: Option<u32>,
    work_invalid: bool,
    rest_secs: Option<u32>,
    rest_after_secs: Option<u32>,
    color: Option<String>,
}

impl BlockBuilder {
    fn set_param(&mut self, key: &str, val: &str, line: usize, errors: &mut Vec<ParseError>) {
        let dur = |errors: &mut Vec<ParseError>| match parse_duration(val) {
            Ok(s) => Some(s),
            Err(e) => {
                errors.push(err(line, e));
                None
            }
        };
        let dup = |field: &str, errors: &mut Vec<ParseError>| {
            errors.push(err(line, format!("duplicate '{field}' in block '{}'", self.name)));
        };
        match key {
            "intervals" => {
                if self.intervals.is_some() {
                    return dup("intervals", errors);
                }
                match val.parse::<u32>() {
                    Ok(n) if n >= 1 => self.intervals = Some(n),
                    _ => errors.push(err(line, format!("'intervals' must be a whole number >= 1, got '{val}'"))),
                }
            }
            "work" => {
                if self.work_secs.is_some() {
                    return dup("work", errors);
                }
                self.work_secs = dur(errors);
                self.work_invalid = self.work_secs.is_none();
            }
            "rest" => {
                if self.rest_secs.is_some() {
                    return dup("rest", errors);
                }
                self.rest_secs = dur(errors);
            }
            "rest after" => {
                if self.rest_after_secs.is_some() {
                    return dup("rest after", errors);
                }
                self.rest_after_secs = dur(errors);
            }
            "color" => {
                if self.color.is_some() {
                    return dup("color", errors);
                }
                if valid_color(val) {
                    self.color = Some(val.to_string());
                } else {
                    errors.push(err(line, format!("invalid color '{val}' — use #rgb or #rrggbb")));
                }
            }
            _ => unreachable!(),
        }
    }

    fn finish(self, errors: &mut Vec<ParseError>) -> Option<Block> {
        let work_secs = match self.work_secs {
            Some(0) => {
                errors.push(err(self.line, format!("'work' must be > 0 in block '{}'", self.name)));
                return None;
            }
            Some(s) => s,
            None => {
                if !self.work_invalid {
                    errors.push(err(self.line, format!("block '{}' is missing 'work: <time>'", self.name)));
                }
                return None;
            }
        };
        Some(Block {
            name: self.name,
            description_md: self.description.trim().to_string(),
            intervals: self.intervals.unwrap_or(1),
            work_secs,
            rest_secs: self.rest_secs,
            rest_after_secs: self.rest_after_secs,
            color: self.color,
        })
    }
}

const PARAM_KEYS: [&str; 5] = ["intervals", "work", "rest", "rest after", "color"];

/// Parse a markdown workout document.
///
/// Format: `# Title` names the workout, each `## Heading` starts a block,
/// bullet lines of the form `- key: value` with a known key (intervals, work,
/// rest, rest after, color) set that block's parameters, and every other line
/// in the block is free markdown shown on screen while the block runs.
pub fn parse_workout(src: &str) -> Result<Workout, Vec<ParseError>> {
    let mut errors: Vec<ParseError> = Vec::new();
    let mut name: Option<String> = None;
    let mut id: Option<String> = None;
    let mut builders: Vec<BlockBuilder> = Vec::new();

    for (i, raw) in src.lines().enumerate() {
        let line_no = i + 1;
        let trimmed = raw.trim();
        if let Some(h) = trimmed.strip_prefix("## ") {
            builders.push(BlockBuilder {
                name: h.trim().to_string(),
                line: line_no,
                ..Default::default()
            });
        } else if let Some(h) = trimmed.strip_prefix("# ") {
            if name.is_none() && builders.is_empty() {
                name = Some(h.trim().to_string());
            } else {
                errors.push(err(line_no, "unexpected extra '#' title — use '## Name' for exercise blocks"));
            }
        } else if builders.is_empty() {
            // Preamble, between the title and the first block. The document id
            // is the only thing recognised here; everything else stays ignored.
            if let Some((key, val)) = ids::bullet(trimmed) {
                if key == "id" {
                    if id.is_some() {
                        errors.push(err(line_no, "duplicate 'id'"));
                    } else if ids::valid_uuid(val) {
                        id = Some(val.to_string());
                    } else {
                        errors.push(err(line_no, format!("invalid id '{val}' — expected a UUID")));
                    }
                }
            }
        } else if let Some(b) = builders.last_mut() {
            if let Some((key, val)) = ids::bullet(trimmed) {
                if PARAM_KEYS.contains(&key.as_str()) {
                    b.set_param(&key, val, line_no, &mut errors);
                    continue;
                }
            }
            b.description.push_str(raw);
            b.description.push('\n');
        }
    }

    let name = match name {
        Some(n) => n,
        None => {
            errors.push(err(1, "missing workout title — start the document with '# My Workout'"));
            String::new()
        }
    };
    if builders.is_empty() {
        errors.push(err(1, "no exercise blocks — add at least one '## Exercise' section"));
    }
    let blocks: Vec<Block> = builders.into_iter().filter_map(|b| b.finish(&mut errors)).collect();

    if errors.is_empty() {
        Ok(Workout { id, name, blocks })
    } else {
        errors.sort_by_key(|e| e.line);
        Err(errors)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const FULL: &str = "\
# Monday Squats

## Back Squat
- intervals: 5
- work: 2:00
- rest: 1:30
- rest after: 3:00
- color: #e11d48

Cues: brace hard, hit depth, drive up fast.

## Bench Press
- intervals: 3
- work: 90
";

    const UUID: &str = "9f2c8e1a-4b7d-4c2e-9a11-6f0d3e5b8c74";

    #[test]
    fn reads_the_document_id_from_the_preamble() {
        let w = parse_workout(&format!("# W\n- id: {UUID}\n\n## A\n- work: 30\n")).unwrap();
        assert_eq!(w.id.as_deref(), Some(UUID));
    }

    #[test]
    fn id_is_optional() {
        assert_eq!(parse_workout("# W\n\n## A\n- work: 30\n").unwrap().id, None);
    }

    #[test]
    fn malformed_id_is_an_error() {
        let errs = parse_workout("# W\n- id: nope\n\n## A\n- work: 30\n").unwrap_err();
        assert_eq!(errs[0].line, 2);
        assert!(errs[0].message.contains("invalid id"), "{}", errs[0].message);
    }

    #[test]
    fn duplicate_id_is_an_error() {
        let src = format!("# W\n- id: {UUID}\n- id: {UUID}\n\n## A\n- work: 30\n");
        let errs = parse_workout(&src).unwrap_err();
        assert!(errs[0].message.contains("duplicate 'id'"), "{}", errs[0].message);
    }

    #[test]
    fn an_id_inside_a_block_stays_prose() {
        // Same rule as every other unrecognised bullet: identity is a preamble
        // param, so this must not be mistaken for the document id.
        let w = parse_workout(&format!("# W\n\n## A\n- work: 30\n- id: {UUID}\n")).unwrap();
        assert_eq!(w.id, None);
        assert!(w.blocks[0].description_md.contains("- id:"));
    }

    #[test]
    fn serializer_round_trips_the_id() {
        let w = parse_workout(&format!("# W\n- id: {UUID}\n\n## A\n- work: 1:30\n")).unwrap();
        let md = workout_to_markdown(&w);
        assert!(md.starts_with(&format!("# W\n- id: {UUID}\n")), "{md}");
        assert_eq!(parse_workout(&md).unwrap(), w);
    }

    #[test]
    fn parses_full_document() {
        let w = parse_workout(FULL).unwrap();
        assert_eq!(w.name, "Monday Squats");
        assert_eq!(w.blocks.len(), 2);
        let b = &w.blocks[0];
        assert_eq!(b.name, "Back Squat");
        assert_eq!(b.intervals, 5);
        assert_eq!(b.work_secs, 120);
        assert_eq!(b.rest_secs, Some(90));
        assert_eq!(b.rest_after_secs, Some(180));
        assert_eq!(b.color.as_deref(), Some("#e11d48"));
        assert_eq!(b.description_md, "Cues: brace hard, hit depth, drive up fast.");
        let b2 = &w.blocks[1];
        assert_eq!(b2.intervals, 3);
        assert_eq!(b2.work_secs, 90);
        assert_eq!(b2.rest_secs, None);
    }

    #[test]
    fn intervals_default_to_one() {
        let w = parse_workout("# W\n\n## A\n- work: 30\n").unwrap();
        assert_eq!(w.blocks[0].intervals, 1);
    }

    #[test]
    fn unknown_bullet_keys_stay_in_description() {
        let w = parse_workout("# W\n\n## A\n- work: 30\n- weight: 80kg\n- focus on form\n").unwrap();
        assert!(w.blocks[0].description_md.contains("weight: 80kg"));
        assert!(w.blocks[0].description_md.contains("focus on form"));
    }

    #[test]
    fn missing_title_and_blocks() {
        let errs = parse_workout("just some text\n").unwrap_err();
        assert!(errs.iter().any(|e| e.message.contains("missing workout title")));
        assert!(errs.iter().any(|e| e.message.contains("no exercise blocks")));
    }

    #[test]
    fn missing_work_is_an_error() {
        let errs = parse_workout("# W\n\n## A\n- intervals: 3\n").unwrap_err();
        assert_eq!(errs.len(), 1);
        assert_eq!(errs[0].line, 3);
        assert!(errs[0].message.contains("missing 'work"));
    }

    #[test]
    fn bad_time_reports_line() {
        let errs = parse_workout("# W\n\n## A\n- work: 1:75\n").unwrap_err();
        assert_eq!(errs[0].line, 4);
        assert!(errs[0].message.contains("invalid time"));
    }

    #[test]
    fn duplicate_param_is_an_error() {
        let errs = parse_workout("# W\n\n## A\n- work: 30\n- work: 40\n").unwrap_err();
        assert!(errs[0].message.contains("duplicate 'work'"));
    }

    #[test]
    fn zero_work_is_an_error() {
        let errs = parse_workout("# W\n\n## A\n- work: 0\n").unwrap_err();
        assert!(errs[0].message.contains("must be > 0"));
    }

    #[test]
    fn extra_h1_is_an_error() {
        let errs = parse_workout("# W\n# Again\n\n## A\n- work: 30\n").unwrap_err();
        assert_eq!(errs[0].line, 2);
    }

    #[test]
    fn duration_formats() {
        assert_eq!(parse_duration("90").unwrap(), 90);
        assert_eq!(parse_duration("1:30").unwrap(), 90);
        assert_eq!(parse_duration("1:00:05").unwrap(), 3605);
        assert!(parse_duration("1:75").is_err());
        assert!(parse_duration("abc").is_err());
        assert!(parse_duration("").is_err());
        assert!(parse_duration("1:2:3:4").is_err());
    }

    #[test]
    fn renders_markdown() {
        let html = render_markdown("**bold** cue");
        assert!(html.contains("<strong>bold</strong>"));
    }

    #[test]
    fn serializer_round_trips_full_document() {
        let w = parse_workout(FULL).unwrap();
        let md = workout_to_markdown(&w);
        assert_eq!(parse_workout(&md).unwrap(), w);
    }

    #[test]
    fn serializer_round_trips_minimal_and_multiline_notes() {
        let src = "# W\n\n## A\n- work: 30\n\nline one\n\n- a note bullet\nline two\n\n## B\n- intervals: 2\n- work: 45\n- rest: 10\n";
        let w = parse_workout(src).unwrap();
        let md = workout_to_markdown(&w);
        assert_eq!(parse_workout(&md).unwrap(), w);
    }

    #[test]
    fn serializer_output_shape() {
        let w = parse_workout("# W\n\n## A\n- intervals: 2\n- work: 90\n- rest after: 120\n").unwrap();
        let md = workout_to_markdown(&w);
        assert!(md.contains("# W\n"));
        assert!(md.contains("## A\n"));
        assert!(md.contains("- work: 1:30\n"));
        assert!(md.contains("- rest after: 2:00\n"));
        assert!(!md.contains("- rest:"));
        assert!(!md.contains("- color:"));
    }

    #[test]
    fn fmt_secs_formats() {
        assert_eq!(fmt_secs(90), "1:30");
        assert_eq!(fmt_secs(5), "0:05");
        assert_eq!(fmt_secs(3700), "61:40");
    }
}
