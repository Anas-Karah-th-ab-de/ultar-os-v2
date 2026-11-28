#!/usr/bin/env python3
"""Builds word-style.docx: a small document written the way Microsoft Word 2016 writes its
files (rsid attributes, mc:Ignorable, multi-level numbering, a hyperlink, a Word page break,
a tracked insertion, a merged table cell, a header part and a Letter section).

It is assembled by hand, NOT exported by Word or LibreOffice (neither is available on the
build machines); it exists so that the reader is tested against Word's dialect and not only
against the files our own writer produces. Run: python3 make_word_style.py word-style.docx
"""
import sys
import zipfile

W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main"'
R = 'xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships"'
MC = 'xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006"'
W14 = 'xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml"'
HEAD = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'

content_types = HEAD + (
    '<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">'
    '<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>'
    '<Default Extension="xml" ContentType="application/xml"/>'
    '<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>'
    '<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>'
    '<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>'
    '<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>'
    '</Types>'
)

rels = HEAD + (
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/>'
    '</Relationships>'
)

doc_rels = HEAD + (
    '<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
    '<Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/styles" Target="styles.xml"/>'
    '<Relationship Id="rId2" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/numbering" Target="numbering.xml"/>'
    '<Relationship Id="rId3" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/header" Target="header1.xml"/>'
    '<Relationship Id="rId4" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/hyperlink" Target="https://code-craft.tech/" TargetMode="External"/>'
    '</Relationships>'
)

styles = HEAD + (
    f'<w:styles {W}>'
    '<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:asciiTheme="minorHAnsi" w:hAnsiTheme="minorHAnsi" w:cstheme="minorBidi"/>'
    '<w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-US" w:eastAsia="en-US" w:bidi="ar-SA"/></w:rPr></w:rPrDefault>'
    '<w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>'
    '<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>'
    '<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="10"/><w:qFormat/>'
    '<w:rPr><w:rFonts w:asciiTheme="majorHAnsi"/><w:spacing w:val="-10"/><w:kern w:val="28"/><w:sz w:val="56"/><w:szCs w:val="56"/></w:rPr></w:style>'
    '<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:uiPriority w:val="9"/><w:qFormat/>'
    '<w:pPr><w:keepNext/><w:keepLines/><w:spacing w:before="240" w:after="0"/><w:outlineLvl w:val="0"/></w:pPr>'
    '<w:rPr><w:sz w:val="32"/><w:szCs w:val="32"/></w:rPr></w:style>'
    '<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:uiPriority w:val="34"/><w:qFormat/>'
    '<w:pPr><w:ind w:left="720"/><w:contextualSpacing/></w:pPr></w:style>'
    '<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style>'
    '</w:styles>'
)

numbering = HEAD + (
    f'<w:numbering {W}>'
    '<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/>'
    '<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>'
    '<w:lvl w:ilvl="1"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="o"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="1440" w:hanging="360"/></w:pPr></w:lvl>'
    '</w:abstractNum>'
    '<w:abstractNum w:abstractNumId="1"><w:multiLevelType w:val="hybridMultilevel"/>'
    '<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="decimal"/><w:lvlText w:val="%1."/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr></w:lvl>'
    '</w:abstractNum>'
    '<w:num w:numId="3"><w:abstractNumId w:val="0"/></w:num>'
    '<w:num w:numId="4"><w:abstractNumId w:val="1"/></w:num>'
    '</w:numbering>'
)

header = HEAD + (
    f'<w:hdr {W}><w:p><w:r><w:t>Header text of the fixture</w:t></w:r></w:p></w:hdr>'
)

RS = 'w:rsidR="00A12B34" w:rsidRDefault="00A12B34"'
body = (
    # Arabic title, right-to-left paragraph
    f'<w:p {RS}><w:pPr><w:pStyle w:val="Title"/><w:bidi/><w:jc w:val="right"/></w:pPr>'
    '<w:r><w:rPr><w:rFonts w:hint="cs"/><w:rtl/><w:lang w:bidi="ar-SA"/></w:rPr><w:t>تقرير المشروع</w:t></w:r></w:p>'
    f'<w:p {RS}><w:pPr><w:pStyle w:val="Heading1"/></w:pPr><w:r><w:t>Overview</w:t></w:r></w:p>'
    # runs: plain, bold, italic+underline, highlight, colour+size
    f'<w:p {RS}><w:r><w:t xml:space="preserve">Hello </w:t></w:r>'
    '<w:r><w:rPr><w:b/><w:bCs/></w:rPr><w:t>world</w:t></w:r>'
    '<w:r><w:t xml:space="preserve"> and </w:t></w:r>'
    '<w:r><w:rPr><w:i/><w:iCs/><w:u w:val="single"/></w:rPr><w:t>styled</w:t></w:r>'
    '<w:r><w:t xml:space="preserve"> </w:t></w:r>'
    '<w:r><w:rPr><w:highlight w:val="yellow"/></w:rPr><w:t>marked</w:t></w:r>'
    '<w:r><w:t xml:space="preserve"> </w:t></w:r>'
    '<w:r><w:rPr><w:color w:val="FF0000"/><w:sz w:val="28"/><w:szCs w:val="28"/></w:rPr><w:t>red big</w:t></w:r></w:p>'
    # hyperlink
    f'<w:p {RS}><w:r><w:t xml:space="preserve">See </w:t></w:r>'
    '<w:hyperlink r:id="rId4" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t>the site</w:t></w:r></w:hyperlink></w:p>'
    # bullets and numbers
    f'<w:p {RS}><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr></w:pPr><w:r><w:t>first bullet</w:t></w:r></w:p>'
    f'<w:p {RS}><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="3"/></w:numPr></w:pPr><w:r><w:t>second bullet</w:t></w:r></w:p>'
    f'<w:p {RS}><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="4"/></w:numPr></w:pPr><w:r><w:t>step one</w:t></w:r></w:p>'
    f'<w:p {RS}><w:pPr><w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="4"/></w:numPr></w:pPr><w:r><w:t>step two</w:t></w:r></w:p>'
    # Word's "insert page break": a paragraph with only the break
    f'<w:p {RS}><w:r><w:br w:type="page"/></w:r></w:p>'
    f'<w:p {RS}><w:r><w:t>After the break</w:t></w:r></w:p>'
    # table: merged header cell, then two cells
    '<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="0" w:type="auto"/></w:tblPr>'
    '<w:tblGrid><w:gridCol w:w="4680"/><w:gridCol w:w="4680"/></w:tblGrid>'
    '<w:tr><w:tc><w:tcPr><w:tcW w:w="9360" w:type="dxa"/><w:gridSpan w:val="2"/></w:tcPr><w:p><w:r><w:t>Header</w:t></w:r></w:p></w:tc></w:tr>'
    '<w:tr><w:tc><w:tcPr><w:tcW w:w="4680" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>a</w:t></w:r></w:p></w:tc>'
    '<w:tc><w:tcPr><w:tcW w:w="4680" w:type="dxa"/></w:tcPr><w:p><w:r><w:t>b</w:t></w:r></w:p></w:tc></w:tr></w:tbl>'
    # a tracked insertion
    f'<w:p {RS}><w:r><w:t xml:space="preserve">Kept </w:t></w:r>'
    '<w:ins w:id="1" w:author="Reviewer" w:date="2026-11-20T10:00:00Z"><w:r><w:t>inserted text</w:t></w:r></w:ins></w:p>'
    # Letter section with a header reference
    '<w:sectPr><w:headerReference w:type="default" r:id="rId3"/>'
    '<w:pgSz w:w="12240" w:h="15840"/><w:pgMar w:top="1440" w:right="1440" w:bottom="1440" w:left="1440" w:header="720" w:footer="720" w:gutter="0"/>'
    '<w:cols w:space="720"/></w:sectPr>'
)

document = HEAD + (
    f'<w:document {W} {R} {MC} {W14} mc:Ignorable="w14"><w:body>{body}</w:body></w:document>'
)

out = sys.argv[1] if len(sys.argv) > 1 else "word-style.docx"
with zipfile.ZipFile(out, "w", zipfile.ZIP_DEFLATED) as z:
    z.writestr("[Content_Types].xml", content_types)
    z.writestr("_rels/.rels", rels)
    z.writestr("word/document.xml", document)
    z.writestr("word/_rels/document.xml.rels", doc_rels)
    z.writestr("word/styles.xml", styles)
    z.writestr("word/numbering.xml", numbering)
    z.writestr("word/header1.xml", header)
print("wrote", out)
