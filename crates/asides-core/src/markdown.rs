//! Turns one recording (transcript segments + attached images) into a Markdown entry.

/// A pause longer than this between two transcript segments starts a new paragraph.
const PARAGRAPH_GAP_SECS: f32 = 1.5;

#[derive(Debug, Clone, PartialEq)]
pub struct Segment {
    /// Seconds from the start of the recording.
    pub start: f32,
    pub end: f32,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq)]
pub struct Attachment {
    /// Seconds from the start of the recording at which the image was attached.
    pub offset: f32,
    /// Path relative to the note folder, e.g. `images/03-143851.png`.
    pub rel_path: String,
    /// Alt text, e.g. the wall-clock time `14:38:51`.
    pub label: String,
}

impl Attachment {
    pub fn markdown(&self) -> String {
        format!("![{}]({})", self.label, self.rel_path)
    }
}

/// `## 3 · 14:38` followed by the verbatim transcript. Each image is placed before the first
/// segment that starts at or after the moment it was attached, splitting the paragraph there.
pub fn compose_entry(
    number: u32,
    time_label: &str,
    audio_rel_path: Option<&str>,
    segments: &[Segment],
    attachments: &[Attachment],
) -> String {
    let mut heading = format!("## {number} · {time_label}");
    if let Some(audio) = audio_rel_path {
        heading.push_str(&format!(" · [audio]({audio})"));
    }
    let mut blocks = vec![heading];
    let mut attachments: Vec<&Attachment> = attachments.iter().collect();
    attachments.sort_by(|a, b| a.offset.total_cmp(&b.offset));
    let mut pending = attachments.into_iter().peekable();

    let mut paragraph = String::new();
    let mut last_end: Option<f32> = None;
    for segment in segments {
        let text = segment.text.trim();
        if text.is_empty() {
            continue;
        }
        while let Some(att) = pending.next_if(|a| a.offset <= segment.start) {
            flush(&mut blocks, &mut paragraph);
            blocks.push(att.markdown());
        }
        if last_end.is_some_and(|end| segment.start - end > PARAGRAPH_GAP_SECS) {
            flush(&mut blocks, &mut paragraph);
        }
        if !paragraph.is_empty() {
            paragraph.push(' ');
        }
        paragraph.push_str(text);
        last_end = Some(segment.end);
    }
    flush(&mut blocks, &mut paragraph);
    blocks.extend(pending.map(Attachment::markdown));
    blocks.join("\n\n")
}

/// The plain transcript of a recording, used for titles and for "nothing heard" checks.
pub fn plain_text(segments: &[Segment]) -> String {
    segments.iter().map(|s| s.text.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" ")
}

fn flush(blocks: &mut Vec<String>, paragraph: &mut String) {
    if !paragraph.is_empty() {
        blocks.push(std::mem::take(paragraph));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn seg(start: f32, end: f32, text: &str) -> Segment {
        Segment { start, end, text: format!(" {text}") }
    }

    fn img(offset: f32, n: u32) -> Attachment {
        Attachment { offset, rel_path: format!("images/0{n}.png"), label: format!("img{n}") }
    }

    #[test]
    fn joins_segments_and_breaks_paragraphs_on_long_pauses() {
        let segments = [seg(0.0, 2.0, "One."), seg(2.2, 4.0, "Two."), seg(7.0, 9.0, "Three.")];
        assert_eq!(compose_entry(1, "14:32", None, &segments, &[]), "## 1 · 14:32\n\nOne. Two.\n\nThree.");
    }

    #[test]
    fn places_images_by_timestamp() {
        let segments = [seg(0.0, 2.0, "Here."), seg(2.1, 4.0, "The padding."), seg(4.1, 6.0, "And this.")];
        let attachments = [img(5.0, 3), img(0.0, 1), img(2.05, 2), img(99.0, 4)];
        assert_eq!(
            compose_entry(5, "09:10", Some("audio/05.m4a"), &segments, &attachments),
            "## 5 · 09:10 · [audio](audio/05.m4a)\n\n![img1](images/01.png)\n\nHere.\n\n![img2](images/02.png)\n\nThe padding. And this.\n\n![img3](images/03.png)\n\n![img4](images/04.png)"
        );
    }

    #[test]
    fn images_only_entry() {
        assert_eq!(
            compose_entry(2, "10:00", None, &[seg(0.0, 1.0, "  ")], &[img(1.0, 1)]),
            "## 2 · 10:00\n\n![img1](images/01.png)"
        );
    }

    #[test]
    fn plain_text_trims_segments() {
        assert_eq!(plain_text(&[seg(0.0, 1.0, "A."), seg(1.0, 2.0, ""), seg(2.0, 3.0, "B.")]), "A. B.");
    }
}
