use nu_protocol::Span;
use nu_protocol::engine::StateWorkingSet;

pub fn preparse_overlay_keyword(working_set: &mut StateWorkingSet, spans: &[Span]) {
    if spans.len() >= 2 {
        let first_word = working_set.get_span_contents(spans[0]);
        let second_word = working_set.get_span_contents(spans[1]);
        if first_word == b"overlay"
            && (second_word == b"use" || second_word == b"new" || second_word == b"hide")
        {
            let spans = &spans[2..];
            if second_word == b"use" {
                preparse_overlay_use(working_set, spans)
            } else if second_word == b"hide" {
                preparse_overlay_hide(working_set, spans)
            } else if second_word == b"new" {
                preparse_overlay_new(working_set, spans)
            }
        } else {
            return;
        }
    } else {
        return;
    }
}

fn preparse_overlay_use(working_set: &mut StateWorkingSet, spans: &[Span]) {
    if !spans.is_empty() {
        let first_word = working_set.get_span_contents(spans[0]);
        let name = if spans.len() >= 3 {
            // `overlay use something as another`
            let maybe_as_keyword = working_set.get_span_contents(spans[1]);
            if maybe_as_keyword == b"as" {
                working_set.get_span_contents(spans[2])
            } else {
                return;
            }
        } else {
            // `overlay use something`
            first_word
        };
        working_set.add_preoverlay(name.to_vec());
    }
}

fn preparse_overlay_hide(working_set: &mut StateWorkingSet, spans: &[Span]) {
    match spans.len() {
        0 => {
            // merely `overlay hide`.
            working_set.hide_latest_preoverlay()
        }
        1 => {
            // `overlay hide something`
            let name = working_set.get_span_contents(spans[0]).to_vec();
            working_set.hide_preoverlay(&name);
        }
        _ => return,
    }
}

fn preparse_overlay_new(working_set: &mut StateWorkingSet, spans: &[Span]) {
    if !spans.is_empty() {
        // `overlay new something`
        let name = working_set.get_span_contents(spans[0]);
        working_set.add_preoverlay(name.to_vec())
    }
}
