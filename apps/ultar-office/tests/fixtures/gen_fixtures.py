#!/usr/bin/env python3
"""Makes the two .xlsx fixtures of the spreadsheet tests (run by hand, the results are checked in).

openpyxl-sample.xlsx  written by openpyxl 3.1 (a third-party writer: shared strings, formulas
                      without cached values, styles, widths, frozen panes, right-to-left sheet).
excel-like.xlsx       written by hand in the layout Excel itself uses (relative targets,
                      rich-text shared strings, self-closing <xf/>, cached formula results of
                      every type, a column range, a frozen pane). Modelled on Excel's files; it
                      was NOT saved by Excel.
"""
import sys
import zipfile

from openpyxl import Workbook
from openpyxl.styles import Alignment, Font, PatternFill

out = sys.argv[1] if len(sys.argv) > 1 else "."

wb = Workbook()
ws = wb.active
ws.title = "Umsatz"
ws["A1"], ws["B1"], ws["C1"] = "Monat", "Betrag", "Anteil"
ws["A1"].font = Font(bold=True, color="FFFF0000")
ws["A2"], ws["B2"] = "يناير", 1200.5
ws["A3"], ws["B3"] = "Feb & <März>", 800
ws["A4"], ws["B4"] = "Summe", "=SUM(B2:B3)"
ws["C2"], ws["C3"] = "=B2/$B$4", "=B3/$B$4"
ws["C2"].number_format = "0.0%"
ws["C3"].number_format = "0.0%"
ws["D1"] = 45292  # 2024-01-01
ws["D1"].number_format = "yyyy-mm-dd"
ws["E1"] = True
ws["F1"] = '=IF(B4>1000,"hoch","niedrig")'
ws["A5"] = "gelb"
ws["A5"].fill = PatternFill("solid", start_color="FFFFFF00")
ws["B5"] = "mehr\nZeilen"
ws["B5"].alignment = Alignment(horizontal="center", wrap_text=True)
ws["G1"] = 1234567.891
ws["G1"].number_format = "#,##0.00"
ws.column_dimensions["A"].width = 18
ws.row_dimensions[1].height = 30
ws.freeze_panes = "B2"
ws2 = wb.create_sheet("Kosten")
ws2["A1"] = "=Umsatz!B4/2"
ws2["A2"] = "=SUM(Umsatz!B2:B3)*2"
ws2.sheet_view.rightToLeft = True
wb.save(f"{out}/openpyxl-sample.xlsx")

CT = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types"><Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/><Default Extension="xml" ContentType="application/xml"/><Override PartName="/xl/workbook.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"/><Override PartName="/xl/worksheets/sheet1.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/worksheets/sheet2.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.worksheet+xml"/><Override PartName="/xl/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.styles+xml"/><Override PartName="/xl/sharedStrings.xml" ContentType="application/vnd.openxmlformats-officedocument.spreadsheetml.sharedStrings+xml"/></Types>"""
RELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="xl/workbook.xml"/></Relationships>"""
WB = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<workbook xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><workbookPr defaultThemeVersion="124226"/><bookViews><workbookView xWindow="240" yWindow="105" windowWidth="14805" windowHeight="8010"/></bookViews><sheets><sheet name="Daten" sheetId="1" r:id="rId1"/><sheet name="Zweites" sheetId="2" r:id="rId2"/></sheets><calcPr calcId="124519" fullCalcOnLoad="1"/></workbook>"""
WBRELS = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet1.xml"/><Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/worksheet" Target="worksheets/sheet2.xml"/><Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/><Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/sharedStrings" Target="sharedStrings.xml"/></Relationships>"""
SST = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<sst xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" count="5" uniqueCount="5"><si><t>Name</t></si><si><t>Wert</t></si><si><t>مرحبا</t></si><si><r><rPr><b/><sz val="11"/><rFont val="Calibri"/></rPr><t xml:space="preserve">Fett </t></r><r><t>und normal</t></r></si><si><t xml:space="preserve">  Leerzeichen  </t></si></sst>"""
STY = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<styleSheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><numFmts count="1"><numFmt numFmtId="164" formatCode="#,##0.00\\ &quot;EUR&quot;"/></numFmts><fonts count="2"><font><sz val="11"/><color theme="1"/><name val="Calibri"/><family val="2"/></font><font><b/><i/><sz val="11"/><color rgb="FF0000FF"/><name val="Calibri"/><family val="2"/></font></fonts><fills count="3"><fill><patternFill patternType="none"/></fill><fill><patternFill patternType="gray125"/></fill><fill><patternFill patternType="solid"><fgColor rgb="FFFFFF00"/><bgColor indexed="64"/></patternFill></fill></fills><borders count="1"><border><left/><right/><top/><bottom/><diagonal/></border></borders><cellStyleXfs count="1"><xf numFmtId="0" fontId="0" fillId="0" borderId="0"/></cellStyleXfs><cellXfs count="6"><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0"/><xf numFmtId="0" fontId="1" fillId="0" borderId="0" xfId="0" applyFont="1"/><xf numFmtId="9" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="14" fontId="0" fillId="0" borderId="0" xfId="0" applyNumberFormat="1"/><xf numFmtId="164" fontId="0" fillId="2" borderId="0" xfId="0" applyNumberFormat="1" applyFill="1"/><xf numFmtId="0" fontId="0" fillId="0" borderId="0" xfId="0" applyAlignment="1"><alignment horizontal="center" vertical="center" wrapText="1"/></xf></cellXfs><cellStyles count="1"><cellStyle name="Normal" xfId="0" builtinId="0"/></cellStyles></styleSheet>"""
S1 = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"><dimension ref="A1:E6"/><sheetViews><sheetView tabSelected="1" workbookViewId="0"><pane xSplit="1" ySplit="1" topLeftCell="B2" activePane="bottomRight" state="frozen"/><selection pane="bottomRight" activeCell="B2" sqref="B2"/></sheetView></sheetViews><sheetFormatPr defaultRowHeight="15"/><cols><col min="1" max="2" width="14.7109375" customWidth="1"/><col min="3" max="3" width="9.140625" style="1"/></cols><sheetData><row r="1" spans="1:5"><c r="A1" s="1" t="s"><v>0</v></c><c r="B1" s="1" t="s"><v>1</v></c><c r="C1" t="s"><v>2</v></c><c r="D1" t="s"><v>3</v></c><c r="E1" t="s"><v>4</v></c></row><row r="2" spans="1:5" ht="30" customHeight="1"><c r="A2"><v>10</v></c><c r="B2" s="2"><v>0.255</v></c><c r="C2" s="3"><v>45292</v></c><c r="D2" s="4"><v>1234.5</v></c><c r="E2" s="5" t="s"><v>3</v></c></row><row r="3" spans="1:5"><c r="A3"><v>4</v></c><c r="B3"><f>A2/A4</f><v>#DIV/0!</v></c><c r="C3" t="b"><v>1</v></c><c r="D3" t="e"><v>#N/A</v></c><c r="E3" t="str"><f>"x"&amp;A2</f><v>x10</v></c></row><row r="4"><c r="A4"><v>0</v></c><c r="B4"><f>SUM(A2:A3)</f><v>14</v></c><c r="C4"><f>IF(B4&gt;10,"big","small")</f><v>big</v></c><c r="D4"><f>Zweites!A1+1</f><v>0</v></c></row><row r="6"><c r="A6" t="inlineStr"><is><t>inline</t></is></c></row></sheetData><pageMargins left="0.7" right="0.7" top="0.75" bottom="0.75" header="0.3" footer="0.3"/></worksheet>"""
S2 = """<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<worksheet xmlns="http://schemas.openxmlformats.org/spreadsheetml/2006/main"><dimension ref="A1"/><sheetViews><sheetView rightToLeft="1" workbookViewId="0"/></sheetViews><sheetFormatPr defaultRowHeight="15"/><sheetData><row r="1"><c r="A1"><f>SUM(Daten!A2:A3)</f><v>14</v></c></row></sheetData></worksheet>"""
with zipfile.ZipFile(f"{out}/excel-like.xlsx", "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", CT)
    z.writestr("_rels/.rels", RELS)
    z.writestr("xl/workbook.xml", WB)
    z.writestr("xl/_rels/workbook.xml.rels", WBRELS)
    z.writestr("xl/sharedStrings.xml", SST)
    z.writestr("xl/styles.xml", STY)
    z.writestr("xl/worksheets/sheet1.xml", S1)
    z.writestr("xl/worksheets/sheet2.xml", S2)
print("ok")
