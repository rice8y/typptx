use std::{fs, path::Path, process::Command};
use typptx::{ir::Diagnostic, lower, world::CompilerWorld};

fn convert(path: &Path, root: &Path, options: lower::Options) -> Vec<Diagnostic> {
    let world = CompilerWorld::new(path, Some(root), &[], &[]).unwrap();
    let document = world.compile().unwrap().0;
    let mut p = lower::convert_with_options(&document, &options).unwrap();
    world.locate_diagnostics(&mut p);
    p.diagnostics
}

#[test]
fn nested_math_diagnostics_point_to_the_unsupported_expression_in_an_import() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.typ");
    let part = dir.path().join("part.typ");
    fs::write(
        &input,
        "#set page(width:500pt,height:400pt)\n#include \"part.typ\"",
    )
    .unwrap();
    let line = "  [Before $frac(1, x #h(-1em) y)$ after],";
    fs::write(&part, format!("#table(columns:1,\n{line}\n)\n")).unwrap();
    let diagnostics = convert(&input, dir.path(), lower::Options::default());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    let d = &diagnostics[0];
    assert_eq!(d.element.as_deref(), Some("h"), "{diagnostics:?}");
    let source = d
        .source
        .as_ref()
        .expect("unsupported math retains its source");
    assert_eq!(
        source.file,
        part.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(
        (source.line, source.column),
        (2, line.find("h(-1em)").unwrap() + 1)
    );
    assert!(source.end_column > source.column);
    assert!(d.message.contains("negative or excessive math spacing"));
    let json = serde_json::to_value(d).unwrap();
    assert!(json.get("span").is_none());
    assert_eq!(json["source"]["line"], 2);
}

#[test]
fn cli_reports_unicode_columns_and_keeps_existing_outputs_on_failure() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("日本語.typ");
    let output = dir.path().join("existing.pptx");
    let report = dir.path().join("report.json");
    let line = "$ \"説明\" + x #h(-1em) y $";
    fs::write(&input, format!("// leading comment\r\n{line}\r\n")).unwrap();
    fs::write(&output, "keep this deck").unwrap();
    let result = Command::new(env!("CARGO_BIN_EXE_typptx"))
        .arg(&input)
        .arg("-o")
        .arg(&output)
        .arg("--strict")
        .arg("--report")
        .arg(&report)
        .output()
        .unwrap();
    assert!(!result.status.success());
    assert_eq!(fs::read_to_string(&output).unwrap(), "keep this deck");
    let diagnostics: Vec<Diagnostic> = serde_json::from_slice(&fs::read(&report).unwrap()).unwrap();
    assert!(!diagnostics.is_empty());
    let source = diagnostics[0].source.as_ref().unwrap();
    let column = line[..line.find("h(-1em)").unwrap()].chars().count() + 1;
    assert_eq!((source.line, source.column), (2, column));
    let stderr = String::from_utf8(result.stderr).unwrap();
    assert!(
        stderr.contains(&format!(
            "{}:2:{column}: page 1",
            input.canonicalize().unwrap().display()
        )),
        "{stderr}"
    );
    assert!(stderr.contains("h:"), "{stderr}");
    assert!(!stderr.contains("Location("), "{stderr}");
}

#[test]
fn unsupported_graphics_in_list_bodies_keep_their_call_site() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.typ");
    fs::write(
        &input,
        "// source location\n- First #rect(width:4pt,height:4pt,fill:gradient.radial(red.transparentize(80%),blue))\n- Second",
    )
    .unwrap();
    let diagnostics = convert(&input, dir.path(), lower::Options::default());
    assert!(!diagnostics.is_empty());
    assert_eq!(diagnostics[0].source.as_ref().unwrap().line, 2);
}

#[test]
fn rotated_native_tables_inside_lists_report_office_limitations() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.typ");
    fs::write(
        &input,
        "#rotate(20deg)[\n- Before\n\n  #table(columns:2,[A],[B])\n]",
    )
    .unwrap();
    let diagnostics = convert(&input, dir.path(), lower::Options::default());
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert!(
        diagnostics[0]
            .message
            .contains("rotation or reflection to native tables")
    );
}

#[test]
fn generated_macro_content_reports_its_definition_and_compile_errors_have_columns() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.typ");
    let defs = dir.path().join("defs.typ");
    fs::write(
        &defs,
        "// macro definition\n#let formula() = $x #h(-1em) y$\n",
    )
    .unwrap();
    fs::write(&input, "#import \"defs.typ\": formula\n#formula()").unwrap();
    let diagnostics = convert(&input, dir.path(), lower::Options::default());
    assert!(!diagnostics.is_empty());
    let source = diagnostics[0].source.as_ref().unwrap();
    assert_eq!(
        source.file,
        defs.canonicalize().unwrap().display().to_string()
    );
    assert_eq!(source.line, 2);
    assert_eq!(diagnostics[0].element.as_deref(), Some("h"));
    fs::write(&input, "// first line\n#unknown-function()").unwrap();
    let world = CompilerWorld::new(&input, None, &[], &[]).unwrap();
    let error = world.compile().unwrap_err().to_string();
    assert!(
        error.contains(&format!("{}:2:2:", input.canonicalize().unwrap().display())),
        "{error}"
    );
}

#[test]
fn fallback_reports_keep_sources_and_generated_content_does_not_invent_them() {
    let dir = tempfile::tempdir().unwrap();
    let input = dir.path().join("main.typ");
    fs::write(&input, "$ x #h(-1em) y $").unwrap();
    let diagnostics = convert(
        &input,
        dir.path(),
        lower::Options {
            allow_image_fallback: true,
            ..Default::default()
        },
    );
    assert_eq!(diagnostics.len(), 1, "{diagnostics:?}");
    assert_eq!(diagnostics[0].code, "drawing_fallback");
    assert!(diagnostics[0].source.is_some());
    let world = CompilerWorld::new(&input, None, &[], &[]).unwrap();
    assert!(
        world
            .source_location(typst::syntax::Span::detached())
            .is_none()
    );
    let old: Diagnostic = serde_json::from_str(r#"{"page":1,"source_id":"generated","code":"unsupported_element","message":"unsupported"}"#).unwrap();
    assert!(old.source.is_none());
}
