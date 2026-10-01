//! Read notes from evaluated metadata in the same compiled document.
use anyhow::{Result, anyhow, ensure};
use typst::foundations::{NativeElement, Value};
use typst::introspection::{Introspector, MetadataElem};
use typst_layout::PagedDocument;

pub fn extract(document: &PagedDocument) -> Result<Vec<Option<String>>> {
    let introspector = document.introspector();
    let mut notes = vec![None; document.pages().len()];
    let mut bundled_notes = vec![false; notes.len()];
    // Touying's published metadata assigns notes to physical pages, including
    // overlays. Reading it avoids re-evaluating source or guessing slide IDs.
    for content in introspector.query(&MetadataElem::ELEM.select()) {
        if content
            .label()
            .is_none_or(|label| &*label.resolve() != "pdfpc-file")
        {
            continue;
        }
        let Some(metadata) = content.to_packed::<MetadataElem>() else {
            continue;
        };
        let Value::Dict(file) = &metadata.value else {
            continue;
        };
        let Ok(Value::Array(pages)) = file.get("pages") else {
            continue;
        };
        for value in pages {
            let Value::Dict(page) = value else { continue };
            let Ok(Value::Str(note)) = page.get("note") else {
                continue;
            };
            let Ok(Value::Int(index)) = page.get("idx") else {
                return Err(anyhow!("pdfpc speaker note has no page index"));
            };
            ensure!(
                *index >= 0 && (*index as usize) < notes.len(),
                "pdfpc speaker note page index {index} is outside the document"
            );
            append(&mut notes[*index as usize], note);
            bundled_notes[*index as usize] = true;
        }
    }
    // A small, converter-owned interface also works without a slide package:
    // #metadata((typptx: "speaker-note", text: "Notes for this page"))
    for content in introspector.query(&MetadataElem::ELEM.select()) {
        let metadata = content.to_packed::<MetadataElem>().unwrap();
        let Value::Dict(value) = &metadata.value else {
            continue;
        };
        // Polylux emits individual pdfpc notes. Touying also emits these, so
        // skip them when its assembled pdfpc-file already supplied the note.
        if content
            .label()
            .is_some_and(|label| &*label.resolve() == "pdfpc")
            && matches!(value.get("t"), Ok(Value::Str(tag)) if tag.as_str() == "Note")
        {
            if let Ok(Value::Str(note)) = value.get("v")
                && let Some(position) = content
                    .location()
                    .and_then(|loc| introspector.position(loc))
            {
                let page = position.page.get() - 1;
                if !bundled_notes[page] {
                    append(&mut notes[page], note);
                }
            }
            continue;
        }
        if !matches!(value.get("typptx"), Ok(Value::Str(tag)) if tag.as_str() == "speaker-note") {
            continue;
        }
        let text = match value.get("text") {
            Ok(Value::Str(text)) => text.to_string(),
            Ok(Value::Content(body)) => body.plain_text().to_string(),
            _ => {
                return Err(anyhow!(
                    "typptx speaker-note metadata needs a string or content in `text`"
                ));
            }
        };
        let position = content
            .location()
            .and_then(|loc| introspector.position(loc))
            .ok_or_else(|| anyhow!("speaker note has no page location"))?;
        append(&mut notes[position.page.get() - 1], &text);
    }
    Ok(notes)
}

fn append(slot: &mut Option<String>, text: &str) {
    if text.trim().is_empty() {
        return;
    }
    let note = slot.get_or_insert_default();
    if !note.is_empty() {
        note.push_str("\n\n");
    }
    note.push_str(text);
}
