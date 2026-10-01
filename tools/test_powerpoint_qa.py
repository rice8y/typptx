"""Detector tests use synthetic evidence; they do not claim Office was run."""

import contextlib
import copy
import io
import json
from pathlib import Path
import tempfile
import unittest
import xml.etree.ElementTree as ET
import zipfile

import powerpoint_qa as qa


def package(path, text="EDIT ME", width=140, picture=False, split_runs=False):
    namespaces = " ".join(f'xmlns:{k}="{v}"' for k, v in qa.NS.items())
    runs = "".join(f"<a:r><a:t>{c}</a:t></a:r>" for c in (text if split_runs else [text]))
    table = f'''<a:tbl><a:tblGrid><a:gridCol w="{width * qa.EMU_PER_PT}"/></a:tblGrid>
      <a:tr h="812800"><a:tc><a:txBody><a:p>{runs}</a:p></a:txBody>
      <a:tcPr marL="101600" marR="101600" marT="101600" marB="101600"/>
      </a:tc></a:tr></a:tbl>'''
    with zipfile.ZipFile(path, "w") as output:
        output.writestr("ppt/presentation.xml", f'<p:presentation {namespaces}><p:sldIdLst><p:sldId id="256" r:id="slide"/></p:sldIdLst><p:sldSz cx="9144000" cy="6096000"/></p:presentation>')
        output.writestr("ppt/_rels/presentation.xml.rels", f'<Relationships xmlns="{qa.NS["pr"]}"><Relationship Id="slide" Target="slides/nonstandard-name.xml"/></Relationships>')
        output.writestr("ppt/slides/nonstandard-name.xml", f'<p:sld {namespaces}><p:cSld><p:spTree><mc:AlternateContent><mc:Choice Requires="a"><p:graphicFrame><a:graphic><a:graphicData>{table}</a:graphicData></a:graphic></p:graphicFrame></mc:Choice><mc:Fallback><p:pic/></mc:Fallback></mc:AlternateContent>{"<p:pic/>" if picture else ""}</p:spTree></p:cSld></p:sld>')


def chars(text, y=20, start=10):
    return [{"text": c, "x0": start + i * 10, "x1": start + i * 10 + 9, "top": y, "bottom": y + 20, "size": 20} for i, c in enumerate(text)]


def display(region, glyphs):
    return {"pages": 1, "page_sizes_pt": [[720, 480]], "regions": [qa.observe_region(glyphs, region)]}


class DetectionTests(unittest.TestCase):
    def setUp(self):
        self.directory = tempfile.TemporaryDirectory()
        self.addCleanup(self.directory.cleanup)
        self.path = Path(self.directory.name)
        self.region = {"id": "cell", "slide": 1, "box_pt": [0, 0, 200, 100], "lines": ["AVATAR"]}
        self.manifest = {"schema_version": 1, "slides": 1, "slide_size_pt": [720, 480], "package": {"native_tables": 1, "pictures": 0, "tables": [{"slide": 1, "table": 1, "columns_pt": [140], "rows_pt": [64]}]}, "regions": [self.region]}
        self.original = self.path / "original.pptx"
        package(self.original)
        self.snapshot = qa.pptx_snapshot(self.original)

    def test_relationship_order_and_alternate_content(self):
        self.assertEqual(qa.check_package(self.snapshot, self.manifest), [])
        self.assertEqual(self.snapshot["native_tables"], 1)
        self.assertEqual(self.snapshot["pictures"], 0)
        self.assertEqual(qa.selected_table(self.snapshot, {"slide": 1, "table": 1})["cells"][0][0]["text"], "EDIT ME")

    def test_run_splitting_is_not_an_editability_failure(self):
        target = self.path / "split.pptx"
        package(target, split_runs=True)
        self.assertEqual(qa.compare_tables(self.snapshot, qa.pptx_snapshot(target)), [])

    def test_native_cell_edit_must_survive(self):
        edit = {"slide": 1, "table": 1, "row": 1, "column": 1, "before": "EDIT ME", "after": "EDIT OK"}
        self.assertTrue(qa.compare_tables(self.snapshot, self.snapshot, [edit]))
        target = self.path / "edited.pptx"
        package(target, text="EDIT OK")
        self.assertEqual(qa.compare_tables(self.snapshot, qa.pptx_snapshot(target), [edit]), [])

    def test_unchanged_picture_preview_does_not_replace_native_cells(self):
        replacement = copy.deepcopy(self.snapshot)
        replacement["slides"][0]["tables"] = []
        replacement["native_tables"] = 0
        replacement["pictures"] = 1
        errors = qa.compare_tables(self.snapshot, replacement)
        self.assertTrue(any("native_tables" in e for e in errors))

    def test_track_and_margin_regressions_are_reported(self):
        changed = copy.deepcopy(self.snapshot)
        table = changed["slides"][0]["tables"][0]
        table["columns_pt"][0] += 1
        table["cells"][0][0]["margins_pt"][0] += 2
        errors = qa.compare_tables(self.snapshot, changed)
        self.assertTrue(any("columns_pt" in e for e in errors))
        self.assertTrue(any("margins_pt" in e for e in errors))

    def test_merge_regression_is_reported(self):
        changed = copy.deepcopy(self.snapshot)
        changed["slides"][0]["tables"][0]["cells"][0][0]["span"] = [2, 1]
        self.assertTrue(any("span" in e for e in qa.compare_tables(self.snapshot, changed)))

    def test_line_wrap_regression_is_reported(self):
        good = display(self.region, chars("AVATAR"))
        bad = display(self.region, chars("AVA") + chars("TAR", y=46))
        self.assertEqual(qa.check_display(good, self.manifest), [])
        self.assertTrue(any("lines" in e for e in qa.check_display(bad, self.manifest)))

    def test_origin_shift_is_reported_even_when_text_and_wrap_match(self):
        good = display(self.region, chars("AVATAR"))
        bad = display(self.region, chars("AVATAR", start=11))
        self.assertTrue(any("glyph positions" in e for e in qa.check_display(bad, self.manifest, good)))

    def test_outside_glyph_edge_is_reported(self):
        region = dict(self.region, content_box_pt=[10, 20, 60, 40])
        manifest = dict(self.manifest, regions=[region])
        self.assertTrue(any("bounds" in e for e in qa.check_display(display(region, chars("AVATAR")), manifest)))

    def test_explicit_source_origins_detect_lost_tracking(self):
        region = dict(self.region, first_line_x_pt=[10, 21, 32, 43, 54, 65])
        manifest = dict(self.manifest, regions=[region])
        self.assertTrue(any("glyph origins" in e for e in qa.check_display(display(region, chars("AVATAR")), manifest)))

    def test_renderer_hash_mismatch_is_reported(self):
        evidence = {"application": "Microsoft PowerPoint", "version": "test", "os": "test", "export_method": "local-pdf-export", "artifacts": {"pptx": {"sha256": qa.sha256(self.original)}}}
        self.assertEqual(qa.evidence_errors(evidence, {"pptx": self.original}), [])
        self.original.write_bytes(b"different bytes")
        self.assertTrue(qa.evidence_errors(evidence, {"pptx": self.original}))

    def test_trimmed_spaces_can_be_excluded_from_visible_origin_checks(self):
        region = dict(self.region, first_line_x_pt=[10, 20, 30, 40, 50, 60],
                      ignore_trailing_space_origins=True)
        manifest = dict(self.manifest, regions=[region])
        observed = display(region, chars("AVATAR "))
        self.assertEqual(qa.check_display(observed, manifest), [])
        self.assertEqual(len(observed["regions"][0]["glyphs"][0]), 7)
        region["ignore_trailing_space_origins"] = False
        self.assertTrue(qa.check_display(observed, manifest))

    def test_math_alphabet_serialization_changes_are_not_cell_edits(self):
        def p(text, math=True):
            tag = "m" if math else "a"
            return ET.fromstring(f'<a:p xmlns:a="{qa.NS["a"]}" xmlns:m="{qa.NS["m"]}"><{tag}:r><{tag}:t>{text}</{tag}:t></{tag}:r></a:p>')
        self.assertEqual(qa.paragraph_text(p("𝑥")), qa.paragraph_text(p("x")))
        self.assertNotEqual(qa.paragraph_text(p("𝑥", False)), qa.paragraph_text(p("x", False)))

    def test_package_success_does_not_imply_office_success(self):
        manifest = self.path / "manifest.json"
        manifest.write_text(json.dumps(self.manifest))
        report = self.path / "report.json"
        with contextlib.redirect_stdout(io.StringIO()):
            result = qa.main(["--manifest", str(manifest), "--pptx", str(self.original), "--output", str(report), "--require-office"])
        self.assertEqual(result, 1)
        output = qa.read_json(report)
        self.assertEqual(output["checks"]["package"]["status"], "passed")
        self.assertEqual(output["checks"]["display"]["status"], "skipped")
        self.assertEqual(output["coverage"], "partial")


if __name__ == "__main__":
    unittest.main()
