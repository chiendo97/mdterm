//! Part-of-speech highlighting.
//!
//! Gated behind the `pos` Cargo feature. When enabled, `apply()` colors prose
//! word-spans by their part of speech using a vendored averaged-perceptron
//! tagger (ported from `postagger`) and NLTK's pretrained model.

/// The 9 part-of-speech color categories mdterm distinguishes.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum PosCategory {
    Noun = 0,
    Verb = 1,
    Adjective = 2,
    Adverb = 3,
    Preposition = 4,
    Conjunction = 5,
    Determiner = 6,
    Pronoun = 7,
    Value = 8,
}

const CATEGORY_COUNT: usize = 9;

const ALL_CATEGORY_NAMES: [&str; CATEGORY_COUNT] = [
    "noun",
    "verb",
    "adjective",
    "adverb",
    "preposition",
    "conjunction",
    "determiner",
    "pronoun",
    "value",
];

impl PosCategory {
    fn from_name(name: &str) -> Option<Self> {
        match name.to_ascii_lowercase().as_str() {
            "noun" => Some(Self::Noun),
            "verb" => Some(Self::Verb),
            "adjective" => Some(Self::Adjective),
            "adverb" => Some(Self::Adverb),
            "preposition" => Some(Self::Preposition),
            "conjunction" => Some(Self::Conjunction),
            "determiner" => Some(Self::Determiner),
            "pronoun" => Some(Self::Pronoun),
            "value" => Some(Self::Value),
            _ => None,
        }
    }

    fn bit(self) -> u16 {
        1u16 << (self as u16)
    }
}

/// Dependency-free bitmask over [`PosCategory`].
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct PosCategorySet(u16);

impl PosCategorySet {
    pub fn all() -> Self {
        Self((1u16 << CATEGORY_COUNT) - 1)
    }

    pub fn empty() -> Self {
        Self(0)
    }

    pub fn insert(&mut self, c: PosCategory) {
        self.0 |= c.bit();
    }

    pub fn contains(&self, c: PosCategory) -> bool {
        self.0 & c.bit() != 0
    }

    /// Parse a list of category names. `"all"` yields every category.
    /// Unknown names return an error listing the valid names.
    pub fn from_names(names: &[String]) -> Result<Self, String> {
        if names.iter().any(|n| n.trim().eq_ignore_ascii_case("all")) {
            return Ok(Self::all());
        }
        let mut set = Self::empty();
        for n in names {
            match PosCategory::from_name(n.trim()) {
                Some(c) => set.insert(c),
                None => {
                    return Err(format!(
                        "unknown POS category '{n}'. Valid: {}",
                        ALL_CATEGORY_NAMES.join(", ")
                    ));
                }
            }
        }
        Ok(set)
    }
}

/// Map a Penn-Treebank tag to a color category. Punctuation, interjections,
/// foreign words, list markers, and unknown tags return `None` (uncolored).
pub fn pt_tag_to_category(tag: &str) -> Option<PosCategory> {
    let c = match tag {
        "NN" | "NNS" | "NNP" | "NNPS" => PosCategory::Noun,
        "VB" | "VBD" | "VBG" | "VBN" | "VBP" | "VBZ" | "MD" => PosCategory::Verb,
        "JJ" | "JJR" | "JJS" => PosCategory::Adjective,
        "RB" | "RBR" | "RBS" | "RP" | "WRB" => PosCategory::Adverb,
        "IN" | "TO" => PosCategory::Preposition,
        "CC" => PosCategory::Conjunction,
        "DT" | "PDT" | "EX" | "POS" | "WDT" => PosCategory::Determiner,
        "PRP" | "PRP$" | "WP" | "WP$" => PosCategory::Pronoun,
        "CD" => PosCategory::Value,
        _ => return None,
    };
    Some(c)
}

use std::collections::HashMap;

use flate2::read::GzDecoder;
use serde::Serialize;

const WEIGHTS_GZ: &[u8] = include_bytes!("../pos_model/weights.json.gz");
const CLASSES_TXT: &str = include_str!("../pos_model/classes.txt");
const TAGS_JSON: &str = include_str!("../pos_model/tags.json");

struct AveragedPerceptron {
    feature_weights: HashMap<String, HashMap<String, f32>>,
    classes: Vec<String>,
}

impl AveragedPerceptron {
    fn from_embedded() -> Result<Self, String> {
        let decoder = GzDecoder::new(WEIGHTS_GZ);
        let parsed: HashMap<String, HashMap<String, f64>> =
            serde_json::from_reader(decoder).map_err(|e| format!("parse weights.json.gz: {e}"))?;
        let feature_weights: HashMap<String, HashMap<String, f32>> = parsed
            .into_iter()
            .map(|(feat, inner)| {
                (
                    feat,
                    inner.into_iter().map(|(tag, w)| (tag, w as f32)).collect(),
                )
            })
            .collect();

        let classes: Vec<String> = CLASSES_TXT
            .split('\n')
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .collect();

        Ok(Self {
            feature_weights,
            classes,
        })
    }

    fn predict(&self, features: &HashMap<String, usize>) -> (&str, f32) {
        let mut scores: HashMap<&str, f32> = HashMap::new();
        for (feature, value) in features {
            if let Some(weights) = self.feature_weights.get(feature.as_str())
                && *value != 0
            {
                for (label, weight) in weights {
                    let score = scores.entry(label.as_str()).or_insert(0.0);
                    *score += weight * (*value as f32);
                }
            }
        }
        let class = self
            .classes
            .iter()
            .map(|c| c.as_str())
            .max_by(|a, b| {
                scores
                    .get(a)
                    .unwrap_or(&0.0)
                    .partial_cmp(scores.get(b).unwrap_or(&0.0))
                    .unwrap()
            })
            .unwrap_or("");
        let exp_sum: f32 = scores.values().map(|v| v.exp()).sum();
        let conf = if exp_sum > 0.0 {
            (scores.get(class).copied().unwrap_or(0.0)).exp() / exp_sum
        } else {
            0.0
        };
        (class, conf)
    }
}

#[derive(Serialize)]
pub struct Tag {
    pub word: String,
    pub tag: String,
    pub conf: f32,
}

pub struct PosTagger {
    model: AveragedPerceptron,
    tags: HashMap<String, String>,
}

impl PosTagger {
    /// Parse the embedded gzip-JSON model. Call once and cache.
    pub fn load() -> Result<Self, String> {
        let tags: HashMap<String, String> =
            serde_json::from_str(TAGS_JSON).map_err(|e| format!("parse tags.json: {e}"))?;
        Ok(Self {
            model: AveragedPerceptron::from_embedded()?,
            tags,
        })
    }

    /// Tag a whitespace-separated sentence.
    pub fn tag(&self, sentence: &str) -> Vec<Tag> {
        let tokens: Vec<&str> = sentence.split_whitespace().collect();
        self.assign_tags(tokens)
    }

    fn assign_tags(&self, tokens: Vec<&str>) -> Vec<Tag> {
        let mut prev = "-START-".to_string();
        let mut prev2 = "-START2-".to_string();
        let mut output: Vec<Tag> = Vec::with_capacity(tokens.len());

        let mut context: Vec<String> = Vec::with_capacity(tokens.len() + 4);
        context.push(prev.clone());
        context.push(prev2.clone());
        for tok in &tokens {
            let mapped = if tok.contains('\'') && !tok.starts_with('\'') {
                "!HYPHEN".to_string()
            } else if tok.parse::<usize>().is_ok() && tok.len() == 4 {
                "!YEAR".to_string()
            } else if !tok.is_empty() && tok.as_bytes()[0].is_ascii_digit() {
                "!DIGITS".to_string()
            } else {
                (*tok).to_string()
            };
            context.push(mapped);
        }
        context.push("-END-".to_string());
        context.push("-END2-".to_string());

        let ctx: Vec<&str> = context.iter().map(|s| s.as_str()).collect();

        for (i, token) in tokens.iter().enumerate() {
            let (tag, conf) = if let Some(known) = self.tags.get(*token) {
                (known.clone(), 1.0)
            } else {
                let feats = Self::get_features(i + 2, token, &ctx, &prev, &prev2);
                let (t, c) = self.model.predict(&feats);
                (t.to_string(), c)
            };
            output.push(Tag {
                word: (*token).to_string(),
                tag: tag.clone(),
                conf,
            });
            prev2 = prev;
            prev = tag;
        }
        output
    }

    #[allow(clippy::too_many_arguments)]
    fn get_features(
        i: usize,
        word: &str,
        context: &[&str],
        prev: &str,
        prev2: &str,
    ) -> HashMap<String, usize> {
        let mut f: HashMap<String, usize> = HashMap::new();
        f.insert("bias".to_string(), 1);

        let suffix = if word.chars().count() > 3 {
            let from = word.char_indices().nth_back(2).map(|(b, _)| b).unwrap_or(0);
            &word[from..]
        } else {
            ""
        };
        f.insert(format!("i suffix {suffix}"), 1);

        let pref1 = word
            .chars()
            .nth(1)
            .map(|c| c.to_string())
            .unwrap_or_default();
        f.insert(format!("i pref1 {pref1}"), 1);

        f.insert(format!("i-1 tag {prev}"), 1);
        f.insert(format!("i-2 tag {prev2}"), 1);
        f.insert(format!("i tag+i-2 tag {prev} {prev2}"), 1);
        f.insert(format!("i word {}", context[i]), 1);
        f.insert(format!("i-1 tag+i word {prev} {}", context[i]), 1);
        f.insert(format!("i-1 word {}", context[i - 1]), 1);
        f.insert(format!("i-2 word {}", context[i - 2]), 1);
        f.insert(format!("i+1 word {}", context[i + 1]), 1);
        f.insert(format!("i+2 word {}", context[i + 2]), 1);

        let next = context[i + 1];
        let next_suffix = if next.chars().count() > 3 {
            let from = next.char_indices().nth_back(2).map(|(b, _)| b).unwrap_or(0);
            &next[from..]
        } else {
            ""
        };
        f.insert(format!("i+1 suffix {next_suffix}"), 1);

        let prv = context[i - 1];
        let prv_suffix = if prv.chars().count() > 3 {
            let from = prv.char_indices().nth_back(2).map(|(b, _)| b).unwrap_or(0);
            &prv[from..]
        } else {
            ""
        };
        f.insert(format!("i-1 suffix {prv_suffix}"), 1);

        f
    }
}

use crate::style::StyledSpan;

/// A word token located within a specific span of a line.
#[derive(Clone, Debug)]
pub struct Token {
    /// Index into the line's `spans` vector.
    pub span_idx: usize,
    /// Byte offset where the token starts within `spans[span_idx].text`.
    pub byte_start: usize,
    /// Byte length of the token text.
    pub byte_len: usize,
    /// The token text itself (no surrounding whitespace).
    pub text: String,
}

/// Split a line's spans into non-whitespace word tokens, recording each
/// token's originating span and byte offset. Tokenization matches the
/// perceptron's `split_whitespace` expectation; punctuation stays attached
/// to the adjacent word.
pub fn tokenize_spans(spans: &[StyledSpan]) -> Vec<Token> {
    let mut out = Vec::new();
    for (idx, span) in spans.iter().enumerate() {
        // Walk by Unicode scalar value, not raw byte. A naive
        // `(byte as char).is_whitespace()` reinterprets UTF-8 continuation
        // bytes 0x85 (NEL) and 0xA0 (NBSP) as whitespace and ends a token
        // mid-character — e.g. "✅" (e2 9c 85) panics on `text[0..2]`.
        // `char_indices` yields byte offsets that are always char boundaries.
        let mut token_start: Option<usize> = None;
        for (byte_off, ch) in span.text.char_indices() {
            if ch.is_whitespace() {
                if let Some(start) = token_start {
                    out.push(Token {
                        span_idx: idx,
                        byte_start: start,
                        byte_len: byte_off - start,
                        text: span.text[start..byte_off].to_string(),
                    });
                    token_start = None;
                }
            } else if token_start.is_none() {
                token_start = Some(byte_off);
            }
        }
        if let Some(start) = token_start {
            let end = span.text.len();
            out.push(Token {
                span_idx: idx,
                byte_start: start,
                byte_len: end - start,
                text: span.text[start..].to_string(),
            });
        }
    }
    out
}

use crate::style::{Line, LineMeta};
use crate::theme::Theme;

fn category_color(theme: &Theme, cat: PosCategory) -> crossterm::style::Color {
    match cat {
        PosCategory::Noun => theme.pos_noun,
        PosCategory::Verb => theme.pos_verb,
        PosCategory::Adjective => theme.pos_adjective,
        PosCategory::Adverb => theme.pos_adverb,
        PosCategory::Preposition => theme.pos_preposition,
        PosCategory::Conjunction => theme.pos_conjunction,
        PosCategory::Determiner => theme.pos_determiner,
        PosCategory::Pronoun => theme.pos_pronoun,
        PosCategory::Value => theme.pos_value,
    }
}

/// Color prose word-spans by part of speech.
///
/// - Skips the first `frontmatter_lines` line indices.
/// - Skips lines whose `meta` is `CodeContent` or `DiagramContent`.
/// - Skips spans marked `style.code` (inline code) or `style.link_url` (links).
/// - Preserves all existing style attributes; only sets `fg`.
/// - Only colors words whose category is in `categories`.
#[allow(clippy::ptr_arg)]
pub fn apply(
    lines: &mut Vec<Line>,
    theme: &Theme,
    tagger: &PosTagger,
    categories: PosCategorySet,
    frontmatter_lines: Option<usize>,
) {
    let skip = frontmatter_lines.unwrap_or(0);
    for (line_idx, line) in lines.iter_mut().enumerate() {
        if line_idx < skip {
            continue;
        }
        if matches!(line.meta, LineMeta::CodeContent { .. }) {
            continue;
        }
        // Tokenize, but only non-exempt spans contribute to the sentence.
        let tokens = tokenize_spans(&line.spans);
        if tokens.is_empty() {
            continue;
        }
        // Precompute which spans are exempt (inline code / link) so we both
        // exclude them from the tagging sentence and skip coloring them.
        // Ownership held here so later mutable assignment to `line.spans` is
        // not blocked by an outstanding borrow from the closure.
        let exempt_spans: Vec<bool> = line
            .spans
            .iter()
            .map(|s| s.style.code || s.style.link_url.is_some())
            .collect();
        let is_exempt = |span_idx: usize| exempt_spans[span_idx];
        // Build the sentence from tokens whose originating span is not exempt.
        let sentence: String = tokens
            .iter()
            .filter(|t| !is_exempt(t.span_idx))
            .map(|t| t.text.as_str())
            .collect::<Vec<_>>()
            .join(" ");
        if sentence.is_empty() {
            continue;
        }
        let tagged = tagger.tag(&sentence);

        // Walk tokens in order, assigning each a tag from `tagged` in sequence.
        // Tokens from exempt spans don't consume a tag (they weren't in the sentence).
        let mut tag_iter = tagged.into_iter();
        // Collect recolored spans per span index, then splice back in.
        // Strategy: rebuild each span's contribution by splitting on its tokens.
        let mut new_spans: Vec<StyledSpan> = Vec::with_capacity(line.spans.len());
        // Tokens are in order; iterate spans and consume their tokens via cursor.
        let mut tok_cursor = 0;
        let tokens_len = tokens.len();
        for (span_idx, span) in line.spans.iter().enumerate() {
            // Gather tokens belonging to this span.
            let mut start = 0usize;
            let text_len = span.text.len();
            let mut pieces: Vec<StyledSpan> = Vec::new();
            while tok_cursor < tokens_len && tokens[tok_cursor].span_idx == span_idx {
                let tok = &tokens[tok_cursor];
                // whitespace before the token
                if tok.byte_start > start {
                    pieces.push(StyledSpan {
                        text: span.text[start..tok.byte_start].to_string(),
                        style: span.style.clone(),
                    });
                }
                let tok_text = span.text[tok.byte_start..tok.byte_start + tok.byte_len].to_string();
                let exempt = is_exempt(span_idx);
                let fg = if exempt {
                    span.style.fg
                } else {
                    // consume a tag
                    match tag_iter.next() {
                        Some(t) => match pt_tag_to_category(&t.tag) {
                            Some(cat) if categories.contains(cat) => {
                                Some(category_color(theme, cat))
                            }
                            _ => span.style.fg,
                        },
                        None => span.style.fg,
                    }
                };
                let mut style = span.style.clone();
                style.fg = fg;
                pieces.push(StyledSpan {
                    text: tok_text,
                    style,
                });
                start = tok.byte_start + tok.byte_len;
                tok_cursor += 1;
            }
            // trailing whitespace/remainder
            if start < text_len {
                pieces.push(StyledSpan {
                    text: span.text[start..].to_string(),
                    style: span.style.clone(),
                });
            }
            if pieces.is_empty() {
                // span had no tokens (e.g., pure whitespace or empty) — keep as-is
                new_spans.push(span.clone());
            } else {
                new_spans.extend(pieces);
            }
        }
        line.spans = new_spans;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::style::{Line, LineMeta, Style, StyledSpan};
    use crate::theme::Theme;

    fn span(text: &str) -> StyledSpan {
        StyledSpan {
            text: text.to_string(),
            style: Style::default(),
        }
    }

    fn plain_line(text: &str) -> Line {
        Line {
            spans: vec![StyledSpan {
                text: text.to_string(),
                style: Style::default(),
            }],
            meta: LineMeta::None,
        }
    }

    fn fg_of_first_word(line: &Line) -> Option<crossterm::style::Color> {
        // first non-empty span's fg
        line.spans
            .iter()
            .find(|s| !s.text.trim().is_empty())
            .and_then(|s| s.style.fg)
    }

    #[test]
    fn apply_colors_noun_and_verb_differently() {
        let theme = Theme::dark();
        let tagger = PosTagger::load().expect("load");
        let mut lines = vec![plain_line("the fox runs quickly")];
        apply(&mut lines, &theme, &tagger, PosCategorySet::all(), None);
        // At least two distinct foregrounds appear among the word spans.
        let fgs: std::collections::HashSet<_> = lines[0]
            .spans
            .iter()
            .filter(|s| !s.text.trim().is_empty())
            .map(|s| format!("{:?}", s.style.fg))
            .collect();
        assert!(fgs.len() >= 2, "expected multiple POS colors, got {fgs:?}");
    }

    #[test]
    fn apply_preserves_bold_attribute() {
        let theme = Theme::dark();
        let tagger = PosTagger::load().expect("load");
        let mut lines = vec![Line {
            spans: vec![StyledSpan {
                text: "the fox runs".to_string(),
                style: Style {
                    bold: true,
                    ..Style::default()
                },
            }],
            meta: LineMeta::None,
        }];
        apply(&mut lines, &theme, &tagger, PosCategorySet::all(), None);
        assert!(
            lines[0].spans.iter().all(|s| s.style.bold),
            "bold must survive apply"
        );
    }

    #[test]
    fn apply_skips_inline_code_spans() {
        let theme = Theme::dark();
        let tagger = PosTagger::load().expect("load");
        let code_color = theme.inline_code_fg;
        let mut lines = vec![Line {
            spans: vec![StyledSpan {
                text: "use foo".to_string(),
                style: Style {
                    code: true,
                    fg: Some(code_color),
                    ..Style::default()
                },
            }],
            meta: LineMeta::None,
        }];
        apply(&mut lines, &theme, &tagger, PosCategorySet::all(), None);
        // code span keeps its original fg
        assert_eq!(fg_of_first_word(&lines[0]), Some(code_color));
    }

    #[test]
    fn apply_skips_link_spans() {
        let theme = Theme::dark();
        let tagger = PosTagger::load().expect("load");
        let link_color = theme.link;
        let mut lines = vec![Line {
            spans: vec![StyledSpan {
                text: "click here".to_string(),
                style: Style {
                    fg: Some(link_color),
                    link_url: Some("http://x".to_string()),
                    ..Style::default()
                },
            }],
            meta: LineMeta::None,
        }];
        apply(&mut lines, &theme, &tagger, PosCategorySet::all(), None);
        assert_eq!(fg_of_first_word(&lines[0]), Some(link_color));
    }

    #[test]
    fn apply_skips_code_block_lines() {
        let theme = Theme::dark();
        let tagger = PosTagger::load().expect("load");
        let before = "let x = 1;".to_string();
        let mut lines = vec![Line {
            spans: vec![StyledSpan {
                text: before.clone(),
                style: Style::default(),
            }],
            meta: LineMeta::CodeContent { block_id: 0 },
        }];
        apply(&mut lines, &theme, &tagger, PosCategorySet::all(), None);
        // untouched: still one span with no fg
        assert_eq!(lines[0].spans.len(), 1);
        assert!(lines[0].spans[0].style.fg.is_none());
        assert_eq!(lines[0].spans[0].text, before);
    }

    #[test]
    fn apply_skips_frontmatter_lines() {
        let theme = Theme::dark();
        let tagger = PosTagger::load().expect("load");
        let mut lines = vec![
            plain_line("title: Hello"), // index 0 — frontmatter
            plain_line("the fox runs"), // index 1 — real prose
        ];
        apply(&mut lines, &theme, &tagger, PosCategorySet::all(), Some(1));
        // line 0 untouched (no fg), line 1 colored
        assert!(lines[0].spans[0].style.fg.is_none());
        assert!(lines[1].spans.iter().any(|s| s.style.fg.is_some()));
    }

    #[test]
    fn apply_respects_category_subset() {
        let theme = Theme::dark();
        let tagger = PosTagger::load().expect("load");
        let only_nouns = PosCategorySet::from_names(&["noun".to_string()]).unwrap();
        let mut lines = vec![plain_line("the fox runs quickly")];
        apply(&mut lines, &theme, &tagger, only_nouns, None);
        // Non-noun words keep no fg; at least the noun "fox" gets a color.
        let has_color = lines[0].spans.iter().any(|s| s.style.fg.is_some());
        assert!(has_color, "the noun should be colored");
        let noun_color = theme.pos_noun;
        let fox_colored = lines[0]
            .spans
            .iter()
            .any(|s| s.text.contains("fox") && s.style.fg == Some(noun_color));
        assert!(fox_colored, "'fox' should wear the noun color");
    }

    #[test]
    fn tokenize_single_span_offsets() {
        let toks = tokenize_spans(&[span("the quick fox")]);
        assert_eq!(toks.len(), 3);
        assert_eq!(toks[0].text, "the");
        assert_eq!(
            &"the quick fox"[toks[0].byte_start..toks[0].byte_start + toks[0].byte_len],
            "the"
        );
        assert_eq!(toks[2].text, "fox");
        assert_eq!(
            &"the quick fox"[toks[2].byte_start..toks[2].byte_start + toks[2].byte_len],
            "fox"
        );
    }

    #[test]
    fn tokenize_across_spans_uses_correct_span_idx() {
        // span0: "hello " span1: "world!"
        let toks = tokenize_spans(&[span("hello "), span("world!")]);
        assert_eq!(toks.len(), 2);
        assert_eq!(toks[0].span_idx, 0);
        assert_eq!(toks[0].text, "hello");
        assert_eq!(toks[1].span_idx, 1);
        assert_eq!(toks[1].text, "world!");
        // byte offsets are relative to each token's own span
        assert_eq!(
            &"world!"[toks[1].byte_start..toks[1].byte_start + toks[1].byte_len],
            "world!"
        );
    }

    #[test]
    fn tokenize_skips_pure_whitespace_runs() {
        let toks = tokenize_spans(&[span("   ")]);
        assert!(toks.is_empty());
    }

    #[test]
    fn noun_tags_map_to_noun() {
        assert_eq!(pt_tag_to_category("NN"), Some(PosCategory::Noun));
        assert_eq!(pt_tag_to_category("NNS"), Some(PosCategory::Noun));
        assert_eq!(pt_tag_to_category("NNP"), Some(PosCategory::Noun));
        assert_eq!(pt_tag_to_category("NNPS"), Some(PosCategory::Noun));
    }

    #[test]
    fn verb_tags_include_modals() {
        assert_eq!(pt_tag_to_category("VB"), Some(PosCategory::Verb));
        assert_eq!(pt_tag_to_category("VBD"), Some(PosCategory::Verb));
        assert_eq!(pt_tag_to_category("VBG"), Some(PosCategory::Verb));
        assert_eq!(pt_tag_to_category("VBN"), Some(PosCategory::Verb));
        assert_eq!(pt_tag_to_category("VBP"), Some(PosCategory::Verb));
        assert_eq!(pt_tag_to_category("VBZ"), Some(PosCategory::Verb));
        assert_eq!(pt_tag_to_category("MD"), Some(PosCategory::Verb));
    }

    #[test]
    fn adjective_adverb_preposition_conjunction() {
        assert_eq!(pt_tag_to_category("JJ"), Some(PosCategory::Adjective));
        assert_eq!(pt_tag_to_category("JJR"), Some(PosCategory::Adjective));
        assert_eq!(pt_tag_to_category("JJS"), Some(PosCategory::Adjective));
        assert_eq!(pt_tag_to_category("RB"), Some(PosCategory::Adverb));
        assert_eq!(pt_tag_to_category("RBR"), Some(PosCategory::Adverb));
        assert_eq!(pt_tag_to_category("RBS"), Some(PosCategory::Adverb));
        assert_eq!(pt_tag_to_category("RP"), Some(PosCategory::Adverb));
        assert_eq!(pt_tag_to_category("WRB"), Some(PosCategory::Adverb));
        assert_eq!(pt_tag_to_category("IN"), Some(PosCategory::Preposition));
        assert_eq!(pt_tag_to_category("TO"), Some(PosCategory::Preposition));
        assert_eq!(pt_tag_to_category("CC"), Some(PosCategory::Conjunction));
    }

    #[test]
    fn determiner_pronoun_value() {
        assert_eq!(pt_tag_to_category("DT"), Some(PosCategory::Determiner));
        assert_eq!(pt_tag_to_category("PDT"), Some(PosCategory::Determiner));
        assert_eq!(pt_tag_to_category("EX"), Some(PosCategory::Determiner));
        assert_eq!(pt_tag_to_category("POS"), Some(PosCategory::Determiner));
        assert_eq!(pt_tag_to_category("WDT"), Some(PosCategory::Determiner));
        assert_eq!(pt_tag_to_category("PRP"), Some(PosCategory::Pronoun));
        assert_eq!(pt_tag_to_category("PRP$"), Some(PosCategory::Pronoun));
        assert_eq!(pt_tag_to_category("WP"), Some(PosCategory::Pronoun));
        assert_eq!(pt_tag_to_category("WP$"), Some(PosCategory::Pronoun));
        assert_eq!(pt_tag_to_category("CD"), Some(PosCategory::Value));
    }

    #[test]
    fn punctuation_and_unknown_map_to_none() {
        for tag in [
            ".", ",", ":", "``", "''", "#", "$", "SYM", "UH", "-NONE-", "LS", "FW",
        ] {
            assert_eq!(pt_tag_to_category(tag), None, "tag {tag} should be None");
        }
        assert_eq!(pt_tag_to_category("ZZZ"), None);
    }

    #[test]
    fn category_set_all_contains_every_category() {
        let s = PosCategorySet::all();
        assert!(s.contains(PosCategory::Noun));
        assert!(s.contains(PosCategory::Value));
    }

    #[test]
    fn category_set_from_names_subset() {
        let s = PosCategorySet::from_names(&["noun".to_string(), "verb".to_string()]).unwrap();
        assert!(s.contains(PosCategory::Noun));
        assert!(s.contains(PosCategory::Verb));
        assert!(!s.contains(PosCategory::Adjective));
    }

    #[test]
    fn category_set_unknown_name_errors() {
        let err = PosCategorySet::from_names(&["nown".to_string()]).unwrap_err();
        assert!(err.contains("noun"), "error should list valid names: {err}");
    }

    #[test]
    fn tagger_loads_from_embedded_model() {
        assert!(PosTagger::load().is_ok());
    }

    #[test]
    fn tagger_tags_known_sentence() {
        let t = PosTagger::load().expect("load");
        let tags = t.tag("the quick brown fox jumps over the lazy dog");
        let words: Vec<&str> = tags.iter().map(|x| x.word.as_str()).collect();
        assert_eq!(
            words,
            vec![
                "the", "quick", "brown", "fox", "jumps", "over", "the", "lazy", "dog"
            ]
        );
        let the_tag = tags.iter().find(|x| x.word == "the").unwrap();
        assert_eq!(the_tag.tag, "DT");
        let fox = tags.iter().find(|x| x.word == "fox").unwrap();
        assert!(
            fox.tag.starts_with("NN"),
            "fox should be a noun, got {}",
            fox.tag
        );
    }
    #[test]
    fn tokenize_spans_keeps_multibyte_chars_intact() {
        // ✅ = e2 9c 85: the 0x85 continuation byte is NEL when reinterpreted
        // as a Latin-1 char, which the old byte-walk mistook for whitespace.
        // NBSP (U+00A0 = c2 a0) is the other trap (0xa0 continuation byte).
        let toks = tokenize_spans(&[span("✅ done")]);
        assert_eq!(toks.len(), 2);
        assert_eq!(toks[0].text, "✅");
        assert_eq!(toks[0].byte_start, 0);
        assert_eq!(toks[0].byte_len, 3); // full 3-byte emoji, not split at byte 2
        assert_eq!(toks[1].text, "done");

        let toks2 = tokenize_spans(&[span("a\u{00a0}b")]); // NBSP between words
        assert_eq!(
            toks2.iter().map(|t| t.text.clone()).collect::<Vec<_>>(),
            vec!["a", "b"]
        );
    }

    #[test]
    fn apply_handles_emoji_in_table_and_prose() {
        // Regression: opening a document whose table cells / prose contain
        // emoji with a 0x85/0xa0 UTF-8 continuation byte (e.g. ✅) used to
        // panic with exit 101 when POS highlighting was on.
        use crate::markdown;
        let md = "| a | b |\n|---|---|\n| ✅ Done | 🔜 ⛔ |\n\nSome ✅ prose.\n";
        let theme = Theme::dark();
        let tagger = PosTagger::load().expect("load");
        for w in [40usize, 80, 120, 180, 240] {
            let (mut lines, _doc) = markdown::render(md, w, &theme, false);
            apply(&mut lines, &theme, &tagger, PosCategorySet::all(), None);
            // No panic across any render width.
            let _ = lines;
        }
    }
}
