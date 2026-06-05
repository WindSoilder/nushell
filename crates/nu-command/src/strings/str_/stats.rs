use fancy_regex::Regex;
use nu_engine::command_prelude::*;

use std::collections::BTreeMap;
use std::{fmt, str};
use unicode_segmentation::UnicodeSegmentation;

// borrowed liberally from here https://github.com/dead10ck/uwc
pub type Counted = BTreeMap<Counter, usize>;

#[derive(Clone)]
pub struct StrStats;

impl Command for StrStats {
    fn name(&self) -> &str {
        "str stats"
    }

    fn signature(&self) -> Signature {
        Signature::build("str stats")
            .category(Category::Strings)
            .input_output_types(vec![(Type::String, Type::record())])
    }

    fn description(&self) -> &str {
        "Gather word count statistics on the text."
    }

    fn search_terms(&self) -> Vec<&str> {
        vec!["count", "word", "character", "unicode", "wc"]
    }

    fn is_const(&self) -> bool {
        true
    }

    fn run(
        &self,
        engine_state: &EngineState,
        _stack: &mut Stack,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        stats(engine_state, call, input)
    }

    fn run_const(
        &self,
        working_set: &StateWorkingSet,
        call: &Call,
        input: PipelineData,
    ) -> Result<PipelineData, ShellError> {
        stats(working_set.permanent(), call, input)
    }

    fn examples(&self) -> Vec<Example<'_>> {
        vec![
            Example {
                description: "Count the number of words in a string.",
                example: r#""There are seven words in this sentence" | str stats"#,
                result: Some(Value::test_record(record! {
                        "lines" =>     Value::test_int(1),
                        "words" =>     Value::test_int(7),
                        "bytes" =>     Value::test_int(38),
                        "chars" =>     Value::test_int(38),
                        "graphemes" => Value::test_int(38),
                        "unicode-width" => Value::test_int(38),
                })),
            },
            Example {
                description: "Counts unicode characters.",
                example: "'今天天气真好' | str stats",
                result: Some(Value::test_record(record! {
                        "lines" =>     Value::test_int(1),
                        "words" =>     Value::test_int(6),
                        "bytes" =>     Value::test_int(18),
                        "chars" =>     Value::test_int(6),
                        "graphemes" => Value::test_int(6),
                        "unicode-width" => Value::test_int(12),
                })),
            },
            Example {
                description: "Counts Unicode characters correctly in a string.",
                example: r#""Amélie Amelie" | str stats"#,
                result: Some(Value::test_record(record! {
                        "lines" =>     Value::test_int(1),
                        "words" =>     Value::test_int(2),
                        "bytes" =>     Value::test_int(15),
                        "chars" =>     Value::test_int(14),
                        "graphemes" => Value::test_int(13),
                        "unicode-width" => Value::test_int(13),
                })),
            },
        ]
    }
}

fn stats(
    engine_state: &EngineState,
    call: &Call,
    input: PipelineData,
) -> Result<PipelineData, ShellError> {
    let span = call.head;
    match input {
        // This doesn't match explicit nulls
        PipelineData::Empty => Err(ShellError::PipelineEmpty { dst_span: span }),
        PipelineData::ByteStream(stream, metadata) => {
            Ok(byte_stream_counter(stream, span)?.into_pipeline_data_with_metadata(metadata))
        }
        input => input.map(
            move |v| {
                let value_span = v.span();
                let type_ = v.get_type();
                // First, obtain the span. If this fails, propagate the error that results.
                if let Value::Error { error, .. } = v {
                    return Value::error(*error, span);
                }
                // Now, check if it's a string.
                match v.coerce_into_string() {
                    Ok(s) => counter(&s, span),
                    Err(_) => Value::error(
                        ShellError::OnlySupportsThisInputType {
                            exp_input_type: "string".into(),
                            wrong_type: type_.to_string(),
                            dst_span: span,
                            src_span: value_span,
                        },
                        span,
                    ),
                }
            },
            engine_state.signals(),
        ),
    }
}

fn counter(contents: &str, span: Span) -> Value {
    let counts = uwc_count(&ALL_COUNTERS[..], contents);
    counts_to_value(&counts, span)
}

fn counts_to_value(counts: &BTreeMap<Counter, usize>, span: Span) -> Value {
    fn get_count(counts: &BTreeMap<Counter, usize>, counter: Counter, span: Span) -> Value {
        Value::int(counts.get(&counter).copied().unwrap_or(0) as i64, span)
    }

    let record = record! {
        "lines" => get_count(counts, Counter::Lines, span),
        "words" => get_count(counts, Counter::Words, span),
        "bytes" => get_count(counts, Counter::Bytes, span),
        "chars" => get_count(counts, Counter::CodePoints, span),
        "graphemes" => get_count(counts, Counter::GraphemeClusters, span),
        "unicode-width" => get_count(counts, Counter::UnicodeWidth, span),
    };

    Value::record(record, span)
}

fn byte_stream_counter(stream: ByteStream, span: Span) -> Result<Value, ShellError> {
    let Some(strings) = stream.strings() else {
        return Ok(counter("", span));
    };

    let mut counter = StreamingCounter::default();
    for string in strings {
        counter.push_str(&string?);
    }

    Ok(counts_to_value(&counter.finish(), span))
}

const STREAM_STATS_KEEP_BYTES: usize = 16 * 1024;
const STREAM_STATS_FLUSH_BYTES: usize = STREAM_STATS_KEEP_BYTES * 2;

#[derive(Default)]
struct StreamingCounter {
    pending: String,
    words: usize,
    graphemes: usize,
    unicode_width: usize,
    bytes: usize,
    chars: usize,
    line_counter: StreamingLineCounter,
}

impl StreamingCounter {
    fn push_str(&mut self, value: &str) {
        self.bytes += value.len();
        self.chars += value.chars().count();
        self.line_counter.push_str(value);
        self.pending.push_str(value);
        self.flush_pending(false);
    }

    fn finish(mut self) -> Counted {
        self.flush_pending(true);

        let mut counts = BTreeMap::new();
        counts.insert(Counter::Lines, self.line_counter.finish());
        counts.insert(Counter::Words, self.words);
        counts.insert(Counter::Bytes, self.bytes);
        counts.insert(Counter::CodePoints, self.chars);
        counts.insert(Counter::GraphemeClusters, self.graphemes);
        counts.insert(Counter::UnicodeWidth, self.unicode_width);
        counts
    }

    fn flush_pending(&mut self, finish: bool) {
        let split_at = if finish {
            self.pending.len()
        } else {
            self.safe_split_len()
        };

        if split_at == 0 {
            return;
        }

        let suffix = self.pending.split_off(split_at);
        let prefix = std::mem::replace(&mut self.pending, suffix);
        self.words += prefix.unicode_words().count();
        self.graphemes += prefix.graphemes(true).count();
        self.unicode_width += unicode_width::UnicodeWidthStr::width(prefix.as_str());
    }

    fn safe_split_len(&self) -> usize {
        if self.pending.len() <= STREAM_STATS_FLUSH_BYTES {
            return 0;
        }

        let max_split = floor_char_boundary(
            self.pending.as_str(),
            self.pending.len() - STREAM_STATS_KEEP_BYTES,
        );
        let mut previous_split = 0;
        let mut split = 0;
        let mut grapheme_boundaries = self.pending.grapheme_indices(true).map(|(idx, _)| idx);
        let mut next_grapheme_boundary = grapheme_boundaries.next();

        for (idx, _) in self.pending.split_word_bound_indices() {
            if idx > max_split {
                break;
            }

            while matches!(next_grapheme_boundary, Some(boundary) if boundary < idx) {
                next_grapheme_boundary = grapheme_boundaries.next();
            }

            if next_grapheme_boundary == Some(idx) {
                previous_split = split;
                split = idx;
            }
        }

        if split > 0 && self.pending[..split].ends_with('\r') {
            previous_split
        } else {
            split
        }
    }
}

fn floor_char_boundary(value: &str, mut idx: usize) -> usize {
    while !value.is_char_boundary(idx) {
        idx -= 1;
    }
    idx
}

#[derive(Default)]
struct StreamingLineCounter {
    line_endings: usize,
    saw_any: bool,
    last_was_line_ending: bool,
    pending_cr: bool,
}

impl StreamingLineCounter {
    fn push_str(&mut self, value: &str) {
        for ch in value.chars() {
            self.saw_any = true;

            if self.pending_cr {
                if ch == '\n' {
                    self.line_endings += 1;
                    self.pending_cr = false;
                    self.last_was_line_ending = true;
                    continue;
                }

                self.line_endings += 1;
                self.pending_cr = false;
            }

            match ch {
                '\r' => {
                    self.pending_cr = true;
                    self.last_was_line_ending = true;
                }
                '\n' | '\u{0085}' | '\u{000C}' | '\u{2028}' | '\u{2029}' => {
                    self.line_endings += 1;
                    self.last_was_line_ending = true;
                }
                _ => {
                    self.last_was_line_ending = false;
                }
            }
        }
    }

    fn finish(mut self) -> usize {
        if self.pending_cr {
            self.line_endings += 1;
            self.pending_cr = false;
            self.last_was_line_ending = true;
        }

        if !self.saw_any {
            0
        } else if self.last_was_line_ending {
            self.line_endings
        } else {
            self.line_endings + 1
        }
    }
}

// /// Sums all the `Counted` instances into a new one.
// pub fn sum_all_counts<'a, I>(counts: I) -> Counted
// where
//     I: IntoIterator<Item = &'a Counted>,
// {
//     let mut totals = BTreeMap::new();
//     for counts in counts {
//         sum_counts(&mut totals, counts);
//     }
//     totals
// }

/// Something that counts things in `&str`s.
pub trait Count {
    /// Counts something in the given `&str`.
    fn count(&self, s: &str) -> usize;
}

impl Count for Counter {
    fn count(&self, s: &str) -> usize {
        match *self {
            Counter::GraphemeClusters => s.graphemes(true).count(),
            Counter::Bytes => s.len(),
            Counter::Lines => {
                const LF: &str = "\n"; // 0xe0000a
                const CR: &str = "\r"; // 0xe0000d
                const CRLF: &str = "\r\n"; // 0xe00d0a
                const NEL: &str = "\u{0085}"; // 0x00c285
                const FF: &str = "\u{000C}"; // 0x00000c
                const LS: &str = "\u{2028}"; // 0xe280a8
                const PS: &str = "\u{2029}"; // 0xe280a9

                // use regex here because it can search for CRLF first and not duplicate the count
                let line_ending_types = [CRLF, LF, CR, NEL, FF, LS, PS];
                let pattern = &line_ending_types.join("|");
                let newline_pattern = Regex::new(pattern).expect("Unable to create regex");
                let line_endings = newline_pattern
                    .find_iter(s)
                    .map(|f| match f {
                        Ok(mat) => mat.as_str().to_string(),
                        Err(_) => "".to_string(),
                    })
                    .collect::<Vec<String>>();

                let has_line_ending_suffix =
                    line_ending_types.iter().any(|&suffix| s.ends_with(suffix));
                // eprintln!("suffix = {}", has_line_ending_suffix);

                if has_line_ending_suffix {
                    line_endings.len()
                } else {
                    line_endings.len() + 1
                }
            }
            Counter::Words => s.unicode_words().count(),
            Counter::CodePoints => s.chars().count(),
            Counter::UnicodeWidth => unicode_width::UnicodeWidthStr::width(s),
        }
    }
}

/// Different types of counters.
#[derive(Debug, Hash, PartialEq, Eq, PartialOrd, Ord, Copy, Clone)]
pub enum Counter {
    /// Counts lines.
    Lines,

    /// Counts words.
    Words,

    /// Counts the total number of bytes.
    Bytes,

    /// Counts grapheme clusters. The input is required to be valid UTF-8.
    GraphemeClusters,

    /// Counts unicode code points
    CodePoints,

    /// Counts the width of the string
    UnicodeWidth,
}

/// A convenience array of all counter types.
pub const ALL_COUNTERS: [Counter; 6] = [
    Counter::GraphemeClusters,
    Counter::Bytes,
    Counter::Lines,
    Counter::Words,
    Counter::CodePoints,
    Counter::UnicodeWidth,
];

impl fmt::Display for Counter {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        let s = match *self {
            Counter::GraphemeClusters => "graphemes",
            Counter::Bytes => "bytes",
            Counter::Lines => "lines",
            Counter::Words => "words",
            Counter::CodePoints => "codepoints",
            Counter::UnicodeWidth => "unicode-width",
        };

        write!(f, "{s}")
    }
}

/// Counts the given `Counter`s in the given `&str`.
pub fn uwc_count<'a, I>(counters: I, s: &str) -> Counted
where
    I: IntoIterator<Item = &'a Counter>,
{
    let mut counts: Counted = counters.into_iter().map(|c| (*c, c.count(s))).collect();
    if let Some(lines) = counts.get_mut(&Counter::Lines) {
        if s.is_empty() {
            // If s is empty, indeed, the count is 0
            *lines = 0;
        } else if *lines == 0 && !s.is_empty() {
            // If s is not empty and the count is 0, it means there
            // is a line without a line ending, so let's make it 1
            *lines = 1;
        } else {
            // no change, whatever the count is, is right
        }
    }
    counts
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_examples() -> nu_test_support::Result {
        nu_test_support::test().examples(StrStats)
    }
}

#[test]
fn test_one_newline() {
    let s = "\n".to_string();
    let counts = uwc_count(&ALL_COUNTERS[..], &s);
    let mut correct_counts = BTreeMap::new();
    correct_counts.insert(Counter::Lines, 1);
    correct_counts.insert(Counter::Words, 0);
    correct_counts.insert(Counter::GraphemeClusters, 1);
    correct_counts.insert(Counter::Bytes, 1);
    correct_counts.insert(Counter::CodePoints, 1);
    correct_counts.insert(Counter::UnicodeWidth, 1);

    assert_eq!(correct_counts, counts);
}

#[test]
fn byte_stream_stats_counts_string_stream() {
    let span = Span::test_data();
    let stream = ByteStream::from_iter(
        vec![
            b"foo ".to_vec(),
            "bar\r".as_bytes().to_vec(),
            "\nbaz".as_bytes().to_vec(),
        ],
        span,
        nu_protocol::Signals::empty(),
        ByteStreamType::String,
    );

    let value = byte_stream_counter(stream, span).expect("string stream should be counted");

    assert_eq!(counter("foo bar\r\nbaz", span), value);
}

#[test]
fn byte_stream_stats_rejects_unknown_stream() {
    let span = Span::test_data();
    let stream = ByteStream::from_iter(
        vec![b"valid utf-8".to_vec()],
        span,
        nu_protocol::Signals::empty(),
        ByteStreamType::Unknown,
    );

    let err = byte_stream_counter(stream, span).expect_err("unknown stream should be rejected");

    assert!(matches!(
        err,
        ShellError::OnlySupportsThisInputType {
            exp_input_type,
            wrong_type,
            ..
        } if exp_input_type == "string stream" && wrong_type == "byte stream"
    ));
}

#[test]
fn streaming_counter_matches_whole_string_across_boundaries() {
    let chunks = [
        "foo\r",
        "\nbar Ame",
        "\u{301}lie ",
        "今天天",
        "气真好",
        "\u{2028}done",
    ];
    let whole = chunks.join("");
    let mut counter = StreamingCounter::default();

    for chunk in chunks {
        counter.push_str(chunk);
    }

    assert_eq!(uwc_count(&ALL_COUNTERS[..], &whole), counter.finish());
}

#[test]
fn test_count_counts_lines() {
    // const LF: &str = "\n"; // 0xe0000a
    // const CR: &str = "\r"; // 0xe0000d
    // const CRLF: &str = "\r\n"; // 0xe00d0a
    const NEL: &str = "\u{0085}"; // 0x00c285
    const FF: &str = "\u{000C}"; // 0x00000c
    const LS: &str = "\u{2028}"; // 0xe280a8
    const PS: &str = "\u{2029}"; // 0xe280a9

    // * \r\n is a single grapheme cluster
    // * trailing newlines are counted
    // * NEL is 2 bytes
    // * FF is 1 byte
    // * LS is 3 bytes
    // * PS is 3 bytes
    let mut s = String::from("foo\r\nbar\n\nbaz");
    s += NEL;
    s += "quux";
    s += FF;
    s += LS;
    s += "xi";
    s += PS;
    s += "\n";

    let counts = uwc_count(&ALL_COUNTERS[..], &s);

    let mut correct_counts = BTreeMap::new();
    correct_counts.insert(Counter::Lines, 8);
    correct_counts.insert(Counter::Words, 5);
    correct_counts.insert(Counter::GraphemeClusters, 23);
    correct_counts.insert(Counter::Bytes, 29);

    // one more than grapheme clusters because of \r\n
    correct_counts.insert(Counter::CodePoints, 24);
    correct_counts.insert(Counter::UnicodeWidth, 23);

    assert_eq!(correct_counts, counts);
}

#[test]
fn test_count_counts_words() {
    let i_can_eat_glass = "Μπορῶ νὰ φάω σπασμένα γυαλιὰ χωρὶς νὰ πάθω τίποτα.";
    let s = String::from(i_can_eat_glass);

    let counts = uwc_count(&ALL_COUNTERS[..], &s);

    let mut correct_counts = BTreeMap::new();
    correct_counts.insert(Counter::GraphemeClusters, 50);
    correct_counts.insert(Counter::Lines, 1);
    correct_counts.insert(Counter::Bytes, i_can_eat_glass.len());
    correct_counts.insert(Counter::Words, 9);
    correct_counts.insert(Counter::CodePoints, 50);
    correct_counts.insert(Counter::UnicodeWidth, 50);

    assert_eq!(correct_counts, counts);
}

#[test]
fn test_count_counts_codepoints() {
    // these are NOT the same! One is e + ́́ , and one is é, a single codepoint
    let one = "é";
    let two = "é";

    let counters = [Counter::CodePoints];

    let counts = uwc_count(&counters[..], one);

    let mut correct_counts = BTreeMap::new();
    correct_counts.insert(Counter::CodePoints, 1);

    assert_eq!(correct_counts, counts);

    let counts = uwc_count(&counters[..], two);

    let mut correct_counts = BTreeMap::new();
    correct_counts.insert(Counter::CodePoints, 2);

    assert_eq!(correct_counts, counts);
}
