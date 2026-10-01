#!/usr/bin/env python3
"""Check package structure separately from externally rendered Office evidence.

Only PDF extraction needs pdfplumber. This program never starts an Office app.
"""

from __future__ import annotations

import argparse
import copy
import hashlib
import json
from pathlib import Path
import posixpath
import sys
import unicodedata
import xml.etree.ElementTree as ET
import zipfile

NS = {
    "a": "http://schemas.openxmlformats.org/drawingml/2006/main",
    "p": "http://schemas.openxmlformats.org/presentationml/2006/main",
    "m": "http://schemas.openxmlformats.org/officeDocument/2006/math",
    "r": "http://schemas.openxmlformats.org/officeDocument/2006/relationships",
    "pr": "http://schemas.openxmlformats.org/package/2006/relationships",
    "mc": "http://schemas.openxmlformats.org/markup-compatibility/2006",
}
EMU_PER_PT = 12700


def read_json(path):
    return json.loads(Path(path).read_text(encoding="utf-8-sig"))


def sha256(path):
    return hashlib.sha256(Path(path).read_bytes()).hexdigest()


def active_xml(node):
    """Count the primary Office branch once, excluding its fallback preview."""
    for child in list(node):
        if child.tag == f"{{{NS['mc']}}}AlternateContent":
            branch = child.find("mc:Choice", NS)
            if branch is None:
                branch = child.find("mc:Fallback", NS)
            index = list(node).index(child)
            node.remove(child)
            if branch is not None:
                for descendant in list(branch):
                    active_xml(descendant)
                    node.insert(index, descendant)
                    index += 1
        else:
            active_xml(child)
    return node


def paragraph_text(node):
    parts = []
    for child in node.iter():
        if child.tag == f"{{{NS['a']}}}t":
            parts.append(child.text or "")
        elif child.tag == f"{{{NS['m']}}}t":
            # Office can replace styled ASCII math letters with equivalent
            # Mathematical Alphanumeric Symbols when saving. Normalize only
            # math text; ordinary cell text still uses exact character checks.
            parts.append(unicodedata.normalize("NFKC", child.text or ""))
        elif child.tag == f"{{{NS['a']}}}br":
            parts.append("\n")
    return "".join(parts).replace("\u200b", "")


def pptx_snapshot(path):
    """A semantic snapshot tolerates run splitting and XML prefix changes."""
    with zipfile.ZipFile(path) as package:
        presentation = ET.fromstring(package.read("ppt/presentation.xml"))
        relationships = ET.fromstring(package.read("ppt/_rels/presentation.xml.rels"))
        targets = {r.get("Id"): r.get("Target") for r in relationships}
        size = presentation.find("p:sldSz", NS)
        result = {
            "slide_size_pt": [int(size.get(k)) / EMU_PER_PT for k in ("cx", "cy")],
            "native_tables": 0, "pictures": 0, "math": 0, "slides": [],
        }
        for relation in presentation.findall("p:sldIdLst/p:sldId", NS):
            target = targets[relation.get(f"{{{NS['r']}}}id")]
            part = target.lstrip("/") if target.startswith("/") else posixpath.normpath("ppt/" + target)
            slide = active_xml(ET.fromstring(package.read(part)))
            tables = []
            for table in slide.findall(".//a:tbl", NS):
                rows = table.findall("a:tr", NS)
                cells = []
                for row in rows:
                    cell_row = []
                    for cell in row.findall("a:tc", NS):
                        properties = cell.find("a:tcPr", NS)
                        margins = [
                            int(properties.get(k, default) if properties is not None else default) / EMU_PER_PT
                            for k, default in (("marL", 91440), ("marR", 91440), ("marT", 45720), ("marB", 45720))
                        ]
                        paragraphs = cell.findall("a:txBody/a:p", NS)
                        cell_row.append({
                            "text": "\n".join(paragraph_text(p) for p in paragraphs),
                            "paragraphs": len(paragraphs),
                            "math": len(cell.findall(".//m:oMath", NS)),
                            "span": [int(cell.get(k, "1")) for k in ("gridSpan", "rowSpan")],
                            "continuation": [cell.get(k, "0") in ("1", "true") for k in ("hMerge", "vMerge")],
                            "margins_pt": margins,
                        })
                    cells.append(cell_row)
                tables.append({
                    "columns_pt": [int(c.get("w")) / EMU_PER_PT for c in table.findall("a:tblGrid/a:gridCol", NS)],
                    "rows_pt": [int(r.get("h")) / EMU_PER_PT for r in rows],
                    "cells": cells,
                })
            item = {"tables": tables, "pictures": len(slide.findall(".//p:pic", NS)), "math": len(slide.findall(".//m:oMath", NS))}
            result["slides"].append(item)
            result["native_tables"] += len(tables)
            result["pictures"] += item["pictures"]
            result["math"] += item["math"]
        return result


def selected_table(snapshot, selector):
    return snapshot["slides"][selector["slide"] - 1]["tables"][selector["table"] - 1]


def near_list(actual, expected, tolerance):
    return len(actual) == len(expected) and all(abs(a - e) <= tolerance for a, e in zip(actual, expected))


def check_package(snapshot, manifest):
    errors = []
    if len(snapshot["slides"]) != manifest["slides"]:
        errors.append(f"slide count: {len(snapshot['slides'])} != {manifest['slides']}")
    if "slide_size_pt" in manifest and not near_list(snapshot["slide_size_pt"], manifest["slide_size_pt"], 0.01):
        errors.append("slide size differs")
    expected = manifest.get("package", {})
    for key in ("native_tables", "pictures"):
        if key in expected and snapshot[key] != expected[key]:
            errors.append(f"{key}: {snapshot[key]} != {expected[key]}")
    if snapshot["math"] < expected.get("math_min", 0):
        errors.append("native Office equations are missing")
    for selector in expected.get("tables", []):
        label = f"slide {selector['slide']} table {selector['table']}"
        try:
            table = selected_table(snapshot, selector)
        except IndexError:
            errors.append(f"{label}: missing native table")
            continue
        for dimension in ("columns_pt", "rows_pt"):
            if dimension in selector and not near_list(table[dimension], selector[dimension], 0.02):
                errors.append(f"{label} {dimension}: {table[dimension]} != {selector[dimension]}")
    return errors


def normalize_text(value):
    return unicodedata.normalize("NFC", value).replace("\u200b", "").replace("\xa0", " ")


def observe_region(chars, region):
    """Cluster plain text by top coordinate; avoid this metric for superscripts.

    A glyph belongs to the region when its center lies inside it. Bounds checks
    still inspect the full glyph box, so partial clipping/overflow is reported.
    """
    x0, y0, x1, y1 = region["box_pt"]
    selected = [c for c in chars if x0 <= (c["x0"] + c["x1"]) / 2 < x1 and y0 <= (c["top"] + c["bottom"]) / 2 < y1 and c["size"] >= region.get("min_font_pt", 0)]
    lines = []
    for char in sorted(selected, key=lambda c: (c["top"], c["x0"])):
        line = next((line for line in lines if abs(line[0]["top"] - char["top"]) <= region.get("line_tolerance_pt", 1.5)), None)
        if line is None:
            lines.append([char])
        else:
            line.append(char)
    result = {"id": region["id"], "slide": region["slide"], "lines": [], "glyphs": []}
    for line in lines:
        ordered = sorted(line, key=lambda c: c["x0"])
        result["lines"].append(normalize_text("".join(c["text"] for c in ordered)).strip())
        result["glyphs"].append([{
            "text": normalize_text(c["text"]),
            "x": round(c["x0"], 5), "top": round(c["top"], 5),
            "right": round(c["x1"], 5), "bottom": round(c["bottom"], 5),
        } for c in ordered])
    return result


def pdf_snapshot(path, manifest):
    try:
        import pdfplumber
    except ImportError as exc:
        raise RuntimeError("PDF checks need pdfplumber: python -m pip install -r tools/powerpoint-qa-requirements.txt") from exc
    with pdfplumber.open(path) as pdf:
        return {
            "pages": len(pdf.pages), "metadata": pdf.metadata,
            "page_sizes_pt": [[float(p.width), float(p.height)] for p in pdf.pages],
            "regions": [observe_region(pdf.pages[r["slide"] - 1].chars, r) for r in manifest.get("regions", [])],
        }


def check_display(observed, manifest, baseline=None):
    errors = []
    if observed["pages"] != manifest["slides"]:
        errors.append(f"PDF page count: {observed['pages']} != {manifest['slides']}")
    if "slide_size_pt" in manifest:
        for page, size in enumerate(observed["page_sizes_pt"], 1):
            if not near_list(size, manifest["slide_size_pt"], 0.1):
                errors.append(f"PDF page {page}: wrong page size {size}")
    previous = {r["id"]: r for r in baseline.get("regions", [])} if baseline else {}
    actual = {r["id"]: r for r in observed["regions"]}
    for region in manifest.get("regions", []):
        item = actual.get(region["id"])
        if item is None:
            errors.append(f"{region['id']}: missing PDF observation")
            continue
        if "lines" in region and item["lines"] != [normalize_text(s) for s in region["lines"]]:
            errors.append(f"{region['id']}: lines {item['lines']!r} != {region['lines']!r}")
        if "line_count" in region and len(item["lines"]) != region["line_count"]:
            errors.append(f"{region['id']}: {len(item['lines'])} lines != {region['line_count']}")
        x0, y0, x1, y1 = region.get("content_box_pt", region["box_pt"])
        tolerance = region.get("bounds_tolerance_pt", 0.5)
        if any(g["x"] < x0 - tolerance or g["right"] > x1 + tolerance or g["top"] < y0 - tolerance or g["bottom"] > y1 + tolerance for line in item["glyphs"] for g in line):
            errors.append(f"{region['id']}: glyphs extend beyond content bounds")
        expected_origins = region.get("first_line_x_pt")
        if expected_origins is not None:
            glyphs = list(item["glyphs"][0]) if item["glyphs"] else []
            if region.get("ignore_trailing_space_origins", False):
                while glyphs and glyphs[-1]["text"].isspace():
                    glyphs.pop()
            origins = [g["x"] for g in glyphs]
            if not near_list(origins, expected_origins, region.get("position_tolerance_pt", 0.25)):
                errors.append(f"{region['id']}: glyph origins {origins} != {expected_origins}")
        if baseline is not None:
            before = previous.get(region["id"])
            if before is None:
                errors.append(f"{region['id']}: missing reviewed baseline region")
                continue
            if item["lines"] != before["lines"]:
                errors.append(f"{region['id']}: line breaks changed from reviewed baseline")
                continue
            current = [g for line in item["glyphs"] for g in line]
            reference = [g for line in before["glyphs"] for g in line]
            if [g["text"] for g in current] != [g["text"] for g in reference]:
                errors.append(f"{region['id']}: PDF glyph mapping changed; review the baseline")
            elif any(abs(g[k] - ref[k]) > region.get("position_tolerance_pt", 0.25) for g, ref in zip(current, reference) for k in ("x", "top")):
                errors.append(f"{region['id']}: glyph positions changed from reviewed baseline")
    return errors


def compare_tables(before, after, edits=()):
    """Preserve editable cells, merges, margins and content across Office saves."""
    expected = copy.deepcopy(before)
    errors = []
    for edit in edits:
        try:
            cell = selected_table(expected, edit)["cells"][edit["row"] - 1][edit["column"] - 1]
        except IndexError:
            errors.append(f"edit target missing: {edit}")
            continue
        if cell["text"] != edit["before"]:
            errors.append(f"edit precondition differs: {cell['text']!r} != {edit['before']!r}")
        cell["text"] = edit["after"]
        cell["paragraphs"] = len(edit["after"].split("\n"))
    for key in ("native_tables", "pictures", "math"):
        if expected[key] != after[key]:
            errors.append(f"{key} changed: {expected[key]} -> {after[key]}")
    if not near_list(expected["slide_size_pt"], after["slide_size_pt"], 0.02):
        errors.append("slide size changed")
    if len(expected["slides"]) != len(after["slides"]):
        return errors + ["slide count changed"]
    for slide_index, (left, right) in enumerate(zip(expected["slides"], after["slides"]), 1):
        if len(left["tables"]) != len(right["tables"]):
            errors.append(f"slide {slide_index}: native table count changed")
            continue
        for table_index, (a, b) in enumerate(zip(left["tables"], right["tables"]), 1):
            label = f"slide {slide_index} table {table_index}"
            for key in ("columns_pt", "rows_pt"):
                if not near_list(a[key], b[key], 0.02):
                    errors.append(f"{label}: {key} changed")
            if len(a["cells"]) != len(b["cells"]) or any(len(x) != len(y) for x, y in zip(a["cells"], b["cells"])):
                errors.append(f"{label}: cell grid changed")
                continue
            for row_index, (row_a, row_b) in enumerate(zip(a["cells"], b["cells"]), 1):
                for column_index, (cell_a, cell_b) in enumerate(zip(row_a, row_b), 1):
                    for key in ("text", "paragraphs", "math", "span", "continuation", "margins_pt"):
                        same = near_list(cell_a[key], cell_b[key], 0.02) if key == "margins_pt" else cell_a[key] == cell_b[key]
                        if not same:
                            errors.append(f"{label} cell {row_index},{column_index}: {key} changed")
    return errors


def evidence_errors(evidence, artifacts):
    errors = []
    if evidence.get("application") != "Microsoft PowerPoint":
        errors.append("renderer evidence must identify Microsoft PowerPoint")
    if evidence.get("export_method") not in ("local-pdf-export", "ExportAsFixedFormat"):
        errors.append("renderer evidence must identify a local PDF export")
    for key, path in artifacts.items():
        if path and evidence.get("artifacts", {}).get(key, {}).get("sha256") != sha256(path):
            errors.append(f"renderer evidence does not match {key} bytes")
    if not evidence.get("version") or not evidence.get("os"):
        errors.append("renderer evidence needs Office version and OS")
    return errors


def main(argv=None):
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--manifest", required=True, type=Path)
    parser.add_argument("--pptx", required=True, type=Path)
    parser.add_argument("--pdf", type=Path)
    parser.add_argument("--roundtrip-pptx", type=Path)
    parser.add_argument("--edited-pptx", type=Path)
    parser.add_argument("--roundtrip-pdf", type=Path)
    parser.add_argument("--edited-pdf", type=Path)
    parser.add_argument("--baseline", type=Path, help="Reviewed report/observation JSON; never auto-updated")
    parser.add_argument("--office-evidence", type=Path, help="Operator/runner record with artifact SHA-256 hashes")
    parser.add_argument("--require-office", action="store_true", help="Fail unless display, save/reopen, edit/reopen and matching Office evidence are supplied")
    parser.add_argument("--output", required=True, type=Path)
    args = parser.parse_args(argv)
    manifest = read_json(args.manifest)
    if manifest.get("schema_version") != 1:
        parser.error("unsupported manifest schema_version")
    snapshot = pptx_snapshot(args.pptx)
    report = {"schema_version": 1, "manifest_sha256": sha256(args.manifest), "artifacts": {}, "checks": {}, "package": snapshot}
    artifacts = {"pptx": args.pptx, "pdf": args.pdf, "roundtrip_pptx": args.roundtrip_pptx, "edited_pptx": args.edited_pptx, "roundtrip_pdf": args.roundtrip_pdf, "edited_pdf": args.edited_pdf}
    report["artifacts"] = {key: {"path": str(path), "sha256": sha256(path)} for key, path in artifacts.items() if path}

    def record(name, errors=None):
        report["checks"][name] = {"status": "skipped" if errors is None else "failed" if errors else "passed", "errors": errors or []}

    record("package", check_package(snapshot, manifest))
    if args.pdf:
        report["display"] = pdf_snapshot(args.pdf, manifest)
        baseline = read_json(args.baseline) if args.baseline else None
        if baseline is not None:
            baseline = baseline.get("display", baseline)
        record("display", check_display(report["display"], manifest, baseline))
    else:
        record("display")
    record("save_reopen", compare_tables(snapshot, pptx_snapshot(args.roundtrip_pptx)) if args.roundtrip_pptx else None)
    record("edit_reopen", (compare_tables(snapshot, pptx_snapshot(args.edited_pptx), manifest["edits"]) if manifest.get("edits") else ["manifest declares no edit"]) if args.edited_pptx else None)
    if args.roundtrip_pdf:
        report["roundtrip_display"] = pdf_snapshot(args.roundtrip_pdf, manifest)
        record("save_reopen_display", check_display(report["roundtrip_display"], manifest, report.get("display")))
    else:
        record("save_reopen_display")
    if args.edited_pdf:
        edited_manifest = copy.deepcopy(manifest)
        for edit in manifest.get("edits", []):
            for region in edited_manifest.get("regions", []):
                if region["id"] == edit.get("region"):
                    region["lines"] = edit["after"].split("\n")
                    region.pop("first_line_x_pt", None)
        report["edited_display"] = pdf_snapshot(args.edited_pdf, edited_manifest)
        record("edit_reopen_display", check_display(report["edited_display"], edited_manifest))
    else:
        record("edit_reopen_display")
    if args.office_evidence:
        report["office_evidence"] = read_json(args.office_evidence)
        record("office_evidence", evidence_errors(report["office_evidence"], artifacts))
    else:
        record("office_evidence")
    if args.require_office:
        missing = [name for name, check in report["checks"].items() if check["status"] == "skipped"]
        if not manifest.get("edits"):
            missing.append("at least one declared edit")
        if not manifest.get("regions"):
            missing.append("at least one declared PDF region")
        record("required_office_coverage", [f"missing: {name}" for name in missing])
    report["status"] = "failed" if any(c["status"] == "failed" for c in report["checks"].values()) else "passed"
    report["coverage"] = "complete_office" if all(c["status"] == "passed" for c in report["checks"].values()) else "partial"
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.write_text(json.dumps(report, ensure_ascii=False, indent=2) + "\n", encoding="utf-8")
    for name, check in report["checks"].items():
        print(f"{name}: {check['status']}")
        for error in check["errors"]:
            print(f"  {error}")
    return int(report["status"] == "failed")


if __name__ == "__main__":
    try:
        sys.exit(main())
    except (OSError, ValueError, KeyError, IndexError, zipfile.BadZipFile, ET.ParseError, RuntimeError) as exc:
        print(f"PowerPoint QA: {exc}", file=sys.stderr)
        sys.exit(2)
