//! Compact MOSS transcript parser: `[start][Sxx]text[end]…`
//!
//! Extracts timestamp, speaker, and text from engine raw output.

use super::segment::Segment;

/// Parse a full compact transcript into segments.
pub fn parse_transcript(text: &str) -> Vec<Segment> {
    let mut parser = TranscriptStreamParser::new();
    let mut segments = parser.feed(text);
    segments.extend(parser.close());
    for (i, seg) in segments.iter_mut().enumerate() {
        seg.id = format!("seg_{:04}", i + 1);
    }
    segments
}

#[derive(Debug)]
struct TranscriptStreamParser {
    strip_text: bool,
    skip_empty: bool,
    state: State,
    token: String,
    text: String,
    pending_after_end: String,
    start: Option<f64>,
    end: Option<f64>,
    end_token: String,
    speaker: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum State {
    SeekStart,
    ReadStart,
    ExpectSpeakerOpen,
    ReadSpeaker,
    ReadText,
    ReadEnd,
    AfterEnd,
}

impl TranscriptStreamParser {
    fn new() -> Self {
        Self {
            strip_text: true,
            skip_empty: true,
            state: State::SeekStart,
            token: String::new(),
            text: String::new(),
            pending_after_end: String::new(),
            start: None,
            end: None,
            end_token: String::new(),
            speaker: None,
        }
    }

    fn reset(&mut self) {
        self.state = State::SeekStart;
        self.token.clear();
        self.text.clear();
        self.pending_after_end.clear();
        self.start = None;
        self.end = None;
        self.end_token.clear();
        self.speaker = None;
    }

    fn feed(&mut self, chunk: &str) -> Vec<Segment> {
        let mut out = Vec::new();
        for ch in chunk.chars() {
            self.step(ch, &mut out);
        }
        out
    }

    fn close(&mut self) -> Vec<Segment> {
        let mut out = Vec::new();
        if self.state == State::AfterEnd {
            self.emit_segment(&mut out);
        }
        self.reset();
        out
    }

    fn step(&mut self, ch: char, out: &mut Vec<Segment>) {
        match self.state {
            State::SeekStart => self.seek_start(ch),
            State::ReadStart => self.read_start(ch),
            State::ExpectSpeakerOpen => self.expect_speaker_open(ch),
            State::ReadSpeaker => self.read_speaker(ch),
            State::ReadText => self.read_text(ch),
            State::ReadEnd => self.read_end(ch, out),
            State::AfterEnd => self.after_end(ch, out),
        }
    }

    fn seek_start(&mut self, ch: char) {
        if ch == '[' {
            self.token.clear();
            self.state = State::ReadStart;
        }
    }

    fn read_start(&mut self, ch: char) {
        if ch == ']' {
            match parse_timestamp(&self.token) {
                Some(start) => {
                    self.start = Some(start);
                    self.state = State::ExpectSpeakerOpen;
                    self.token.clear();
                }
                None => {
                    self.reset();
                }
            }
            return;
        }
        if is_timestamp_char(ch) {
            self.token.push(ch);
            if self.token.len() <= 32 {
                return;
            }
        }
        self.reset();
        if ch == '[' {
            self.state = State::ReadStart;
        }
    }

    fn expect_speaker_open(&mut self, ch: char) {
        if ch == '[' {
            self.token.clear();
            self.state = State::ReadSpeaker;
        } else if !ch.is_whitespace() {
            self.reset();
        }
    }

    fn read_speaker(&mut self, ch: char) {
        if ch == ']' {
            match parse_speaker(&self.token) {
                Some(speaker) => {
                    self.speaker = Some(speaker);
                    self.text.clear();
                    self.state = State::ReadText;
                    self.token.clear();
                }
                None => {
                    self.reset();
                }
            }
            return;
        }
        if is_speaker_char(ch) {
            self.token.push(ch);
            if self.token.len() <= 16 {
                return;
            }
        }
        self.reset();
        if ch == '[' {
            self.state = State::ReadStart;
        }
    }

    fn read_text(&mut self, ch: char) {
        if ch == '[' {
            self.token.clear();
            self.state = State::ReadEnd;
        } else {
            self.text.push(ch);
        }
    }

    fn read_end(&mut self, ch: char, _out: &mut Vec<Segment>) {
        if ch == ']' {
            let end = parse_timestamp(&self.token);
            if let (Some(end), Some(start)) = (end, self.start) {
                if end >= start {
                    self.end = Some(end);
                    self.end_token = self.token.clone();
                    self.pending_after_end.clear();
                    self.state = State::AfterEnd;
                    self.token.clear();
                    return;
                }
            }
            self.text.push('[');
            self.text.push_str(&self.token);
            self.text.push(']');
            self.state = State::ReadText;
            self.token.clear();
            return;
        }
        if is_timestamp_char(ch) {
            self.token.push(ch);
            if self.token.len() <= 32 {
                return;
            }
        }
        self.text.push('[');
        self.text.push_str(&self.token);
        self.text.push(ch);
        self.token.clear();
        self.state = State::ReadText;
    }

    fn after_end(&mut self, ch: char, out: &mut Vec<Segment>) {
        if ch == '[' {
            self.emit_segment(out);
            self.token.clear();
            self.state = State::ReadStart;
            return;
        }
        if ch.is_whitespace() {
            self.pending_after_end.push(ch);
            return;
        }
        self.text.push('[');
        self.text.push_str(&self.end_token);
        self.text.push(']');
        self.text.push_str(&self.pending_after_end);
        self.text.push(ch);
        self.pending_after_end.clear();
        self.end = None;
        self.end_token.clear();
        self.state = State::ReadText;
    }

    fn emit_segment(&mut self, out: &mut Vec<Segment>) {
        let (Some(start), Some(end), Some(speaker)) = (self.start, self.end, self.speaker.take()) else {
            self.reset();
            return;
        };
        let mut text = std::mem::take(&mut self.text);
        if self.strip_text {
            text = text.trim().to_string();
        }
        if !text.is_empty() || !self.skip_empty {
            out.push(Segment {
                id: String::new(), // filled by parse_transcript
                start,
                end,
                speaker,
                text,
            });
        }
        self.token.clear();
        self.pending_after_end.clear();
        self.start = None;
        self.end = None;
        self.end_token.clear();
        self.state = State::SeekStart;
    }
}

fn parse_timestamp(s: &str) -> Option<f64> {
    if s.is_empty() {
        return None;
    }
    let mut dots = 0usize;
    let mut digits = 0usize;
    for ch in s.chars() {
        if ch.is_ascii_digit() {
            digits += 1;
        } else if ch == '.' {
            dots += 1;
            if dots > 1 {
                return None;
            }
        } else {
            return None;
        }
    }
    if digits == 0 {
        return None;
    }
    s.parse().ok()
}

fn parse_speaker(s: &str) -> Option<String> {
    let mut chars = s.chars();
    let first = chars.next()?;
    if first != 'S' {
        return None;
    }
    let rest: String = chars.collect();
    if rest.is_empty() || !rest.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    Some(format!("S{rest}"))
}

fn is_timestamp_char(ch: char) -> bool {
    ch.is_ascii_digit() || ch == '.'
}

fn is_speaker_char(ch: char) -> bool {
    ch == 'S' || ch.is_ascii_digit()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_single_segment() {
        let segs = parse_transcript("[1.92][S01]Hello world[8.46]");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].start, 1.92);
        assert_eq!(segs[0].end, 8.46);
        assert_eq!(segs[0].speaker, "S01");
        assert_eq!(segs[0].text, "Hello world");
        assert_eq!(segs[0].id, "seg_0001");
    }

    #[test]
    fn parses_two_adjacent_segments() {
        let raw = "[0.13][S01]first[1.37][1.41][S02]second[2.91]";
        let segs = parse_transcript(raw);
        assert_eq!(segs.len(), 2);
        assert_eq!(segs[0].speaker, "S01");
        assert_eq!(segs[0].text, "first");
        assert_eq!(segs[1].speaker, "S02");
        assert_eq!(segs[1].text, "second");
        assert_eq!(segs[1].start, 1.41);
        assert_eq!(segs[1].end, 2.91);
    }

    #[test]
    fn parses_real_sample_head() {
        let raw = "[1.92][S01]All right, whippers! Welcome everyone.[8.46][8.82][S01]If you are here,[13.08]";
        let segs = parse_transcript(raw);
        assert_eq!(segs.len(), 2);
        assert!(segs[0].text.contains("whippers"));
        assert_eq!(segs[1].start, 8.82);
    }

    #[test]
    fn skips_empty_text() {
        let segs = parse_transcript("[0.0][S01][1.0][1.0][S01]hi[2.0]");
        assert_eq!(segs.len(), 1);
        assert_eq!(segs[0].text, "hi");
    }
}
