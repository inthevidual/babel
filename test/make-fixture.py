#!/usr/bin/env python3
"""Builds test/fixtures/sample.docx: a hand-written OOXML package that exercises
the awkward corners a translation tool has to survive — mixed-format runs, tab
stops, line breaks, lists, a shaded table, footnotes, header/footer with fields,
an inline image, a hyperlink, tracked changes, page breaks and Word's
lastRenderedPageBreak hints. No third-party dependencies."""

import base64, os, struct, zipfile, zlib

W = 'xmlns:w="http://schemas.openxmlformats.org/wordprocessingml/2006/main" xmlns:r="http://schemas.openxmlformats.org/officeDocument/2006/relationships" xmlns:wp="http://schemas.openxmlformats.org/drawingml/2006/wordprocessingDrawing" xmlns:a="http://schemas.openxmlformats.org/drawingml/2006/main" xmlns:pic="http://schemas.openxmlformats.org/drawingml/2006/picture" xmlns:mc="http://schemas.openxmlformats.org/markup-compatibility/2006" xmlns:w14="http://schemas.microsoft.com/office/word/2010/wordml" mc:Ignorable="w14"'
DECL = '<?xml version="1.0" encoding="UTF-8" standalone="yes"?>\n'


def png(w, h):
    raw = b''
    for y in range(h):
        raw += b'\x00' + b''.join(
            bytes((40 + x * 3 % 200, 90 + y * 2 % 150, 160)) for x in range(w))
    def chunk(t, d):
        return struct.pack('>I', len(d)) + t + d + struct.pack('>I', zlib.crc32(t + d) & 0xffffffff)
    return (b'\x89PNG\r\n\x1a\n' + chunk(b'IHDR', struct.pack('>IIBBBBB', w, h, 8, 2, 0, 0, 0))
            + chunk(b'IDAT', zlib.compress(raw)) + chunk(b'IEND', b''))


def r(text, rpr=''):
    return f'<w:r>{f"<w:rPr>{rpr}</w:rPr>" if rpr else ""}<w:t xml:space="preserve">{text}</w:t></w:r>'


def p(*runs, ppr=''):
    return f'<w:p>{f"<w:pPr>{ppr}</w:pPr>" if ppr else ""}{"".join(runs)}</w:p>'


LOREM = ('The committee reviewed the proposal in detail and concluded that the '
         'regional investment should be phased over three budget years, with an '
         'evaluation after each stage. ')

body = []
body.append(p(r('Annual Report on Regional Development'), ppr='<w:pStyle w:val="Title"/>'))
body.append(p(r('1. Introduction'), ppr='<w:pStyle w:val="Heading1"/>'))
body.append(p(
    r('This report is '), r('confidential', '<w:b/>'), r(' and intended for the '),
    r('board of directors', '<w:i/>'), r(' only. It summarises '),
    r('key', '<w:b/><w:i/><w:u w:val="single"/>'), r(' findings from 2025.')))
body.append(p(
    r('See also '),
    '<w:hyperlink r:id="rIdLink" w:history="1"><w:r><w:rPr><w:rStyle w:val="Hyperlink"/></w:rPr><w:t>the project website</w:t></w:r></w:hyperlink>',
    r(' for background material.')))
# Tabs: one run containing text, tab, text (normaliser must split it).
body.append(p(
    '<w:r><w:t>Item</w:t><w:tab/><w:t>Amount</w:t><w:tab/><w:t>Share</w:t></w:r>',
    ppr='<w:tabs><w:tab w:val="left" w:pos="3600"/><w:tab w:val="right" w:pos="8640"/></w:tabs>'))
body.append(p(
    '<w:r><w:t>Infrastructure</w:t><w:tab/><w:t>12 400 000</w:t><w:tab/><w:t>62 %</w:t></w:r>',
    ppr='<w:tabs><w:tab w:val="left" w:pos="3600"/><w:tab w:val="right" w:pos="8640"/></w:tabs>'))
body.append(p(r('First line of an address'), '<w:r><w:br/></w:r>', r('Second line of an address')))
for t in ('Improve public transport links', 'Expand broadband coverage', 'Support local businesses'):
    body.append(p(r(t), ppr='<w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>'))
body.append(p(r('A footnote follows this sentence.'),
              '<w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteReference w:id="1"/></w:r>',
              r(' And the paragraph continues afterwards.')))
# Tracked change: deleted + inserted text.
body.append(p(r('The budget was '),
              '<w:del w:id="90" w:author="Editor" w:date="2025-01-01T00:00:00Z"><w:r><w:delText>reduced</w:delText></w:r></w:del>',
              '<w:ins w:id="91" w:author="Editor" w:date="2025-01-01T00:00:00Z"><w:r><w:t>increased</w:t></w:r></w:ins>',
              r(' after the review.')))
# Complex field (DATE) — result text should stay as is.
body.append(p(r('Printed: '),
              '<w:r><w:fldChar w:fldCharType="begin"/></w:r><w:r><w:instrText xml:space="preserve"> DATE \\@ "yyyy-MM-dd" </w:instrText></w:r><w:r><w:fldChar w:fldCharType="separate"/></w:r>',
              r('2025-06-30'), '<w:r><w:fldChar w:fldCharType="end"/></w:r>'))
# Inline image.
body.append(p(
    '<w:r><w:drawing><wp:inline distT="0" distB="0" distL="0" distR="0"><wp:extent cx="1828800" cy="914400"/>'
    '<wp:docPr id="1" name="Picture 1"/><a:graphic><a:graphicData uri="http://schemas.openxmlformats.org/drawingml/2006/picture">'
    '<pic:pic><pic:nvPicPr><pic:cNvPr id="1" name="chart.png"/><pic:cNvPicPr/></pic:nvPicPr>'
    '<pic:blipFill><a:blip r:embed="rIdImg"/><a:stretch><a:fillRect/></a:stretch></pic:blipFill>'
    '<pic:spPr><a:xfrm><a:off x="0" y="0"/><a:ext cx="1828800" cy="914400"/></a:xfrm><a:prstGeom prst="rect"><a:avLst/></a:prstGeom></pic:spPr>'
    '</pic:pic></a:graphicData></a:graphic></wp:inline></w:drawing></w:r>', ppr='<w:jc w:val="center"/>'))
body.append(p(r('Figure 1. Investment by area.', '<w:i/><w:sz w:val="18"/>'), ppr='<w:jc w:val="center"/>'))
# Table.
def cell(t, shade=None, bold=False):
    tcpr = f'<w:tcPr><w:tcW w:w="3000" w:type="dxa"/>{f"<w:shd w:val=\"clear\" w:color=\"auto\" w:fill=\"{shade}\"/>" if shade else ""}</w:tcPr>'
    return f'<w:tc>{tcpr}{p(r(t, "<w:b/>" if bold else ""))}</w:tc>'
rows = [('Area', 'Budget', 'Status'), ('Transport', '4.2 MSEK', 'On track'),
        ('Broadband', '3.1 MSEK', 'Delayed'), ('Business support', '1.9 MSEK', 'Completed')]
tbl = '<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9000" w:type="dxa"/></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>'
for i, row in enumerate(rows):
    tbl += '<w:tr>' + ''.join(cell(t, 'D9E2F3' if i == 0 else None, i == 0) for t in row) + '</w:tr>'
tbl += '</w:tbl>'
body.append(tbl)
body.append(p(r('2. Analysis'), ppr='<w:pStyle w:val="Heading1"/><w:pageBreakBefore/>'))
for i in range(14):
    if i == 9:
        # Word's own record of where it broke the page, mid-paragraph.
        body.append(p(r(LOREM), '<w:r><w:lastRenderedPageBreak/><w:t xml:space="preserve">' + LOREM + '</w:t></w:r>'))
    else:
        body.append(p(r(f'{i + 1}. ' + LOREM * 2)))
body.append(p('<w:r><w:br w:type="page"/></w:r>'))
body.append(p(r('3. Conclusions'), ppr='<w:pStyle w:val="Heading1"/>'))
body.append(p(r('The regional programme should continue. '), r('Recommendation adopted.', '<w:b/><w:color w:val="C00000"/>')))
body.append(p())
sect = ('<w:sectPr><w:headerReference w:type="default" r:id="rIdHdr"/><w:footerReference w:type="default" r:id="rIdFtr"/>'
        '<w:footnotePr><w:numFmt w:val="decimal"/></w:footnotePr>'
        '<w:pgSz w:w="11906" w:h="16838"/><w:pgMar w:top="1418" w:right="1418" w:bottom="1418" w:left="1418" w:header="709" w:footer="709" w:gutter="0"/></w:sectPr>')
document = f'{DECL}<w:document {W}><w:body>{"".join(body)}{sect}</w:body></w:document>'

header = f'{DECL}<w:hdr {W}>{p(r("Regional Development Agency"), r(" — Annual report", "<w:i/>"), ppr="<w:pStyle w:val=\"Header\"/><w:jc w:val=\"right\"/>")}</w:hdr>'
footer = (f'{DECL}<w:ftr {W}><w:p><w:pPr><w:pStyle w:val="Footer"/><w:jc w:val="center"/></w:pPr>'
          f'{r("Page ")}<w:fldSimple w:instr=" PAGE "><w:r><w:t>1</w:t></w:r></w:fldSimple></w:p></w:ftr>')
footnotes = (f'{DECL}<w:footnotes {W}>'
             '<w:footnote w:type="separator" w:id="-1"><w:p><w:r><w:separator/></w:r></w:p></w:footnote>'
             '<w:footnote w:type="continuationSeparator" w:id="0"><w:p><w:r><w:continuationSeparator/></w:r></w:p></w:footnote>'
             '<w:footnote w:id="1"><w:p><w:pPr><w:pStyle w:val="FootnoteText"/></w:pPr><w:r><w:rPr><w:rStyle w:val="FootnoteReference"/></w:rPr><w:footnoteRef/></w:r>'
             f'{r(" Source: Statistics Sweden, regional accounts 2024.")}</w:p></w:footnote></w:footnotes>')

styles = f'''{DECL}<w:styles {W}>
<w:docDefaults><w:rPrDefault><w:rPr><w:rFonts w:ascii="Calibri" w:hAnsi="Calibri" w:eastAsia="Calibri" w:cs="Times New Roman"/><w:sz w:val="22"/><w:szCs w:val="22"/><w:lang w:val="en-GB" w:eastAsia="en-US" w:bidi="ar-SA"/></w:rPr></w:rPrDefault>
<w:pPrDefault><w:pPr><w:spacing w:after="160" w:line="259" w:lineRule="auto"/></w:pPr></w:pPrDefault></w:docDefaults>
<w:style w:type="paragraph" w:default="1" w:styleId="Normal"><w:name w:val="Normal"/><w:qFormat/></w:style>
<w:style w:type="paragraph" w:styleId="Title"><w:name w:val="Title"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:spacing w:after="240"/></w:pPr><w:rPr><w:rFonts w:ascii="Cambria" w:hAnsi="Cambria"/><w:color w:val="1F3864"/><w:sz w:val="52"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Heading1"><w:name w:val="heading 1"/><w:basedOn w:val="Normal"/><w:next w:val="Normal"/><w:pPr><w:keepNext/><w:spacing w:before="240" w:after="120"/><w:outlineLvl w:val="0"/></w:pPr><w:rPr><w:rFonts w:ascii="Cambria" w:hAnsi="Cambria"/><w:b/><w:color w:val="2F5496"/><w:sz w:val="32"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="ListParagraph"><w:name w:val="List Paragraph"/><w:basedOn w:val="Normal"/><w:pPr><w:ind w:left="720"/><w:contextualSpacing/></w:pPr></w:style>
<w:style w:type="paragraph" w:styleId="Header"><w:name w:val="header"/><w:basedOn w:val="Normal"/><w:rPr><w:color w:val="7F7F7F"/><w:sz w:val="18"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="Footer"><w:name w:val="footer"/><w:basedOn w:val="Normal"/><w:rPr><w:sz w:val="18"/></w:rPr></w:style>
<w:style w:type="paragraph" w:styleId="FootnoteText"><w:name w:val="footnote text"/><w:basedOn w:val="Normal"/><w:pPr><w:spacing w:after="0"/></w:pPr><w:rPr><w:sz w:val="18"/></w:rPr></w:style>
<w:style w:type="character" w:styleId="FootnoteReference"><w:name w:val="footnote reference"/><w:rPr><w:vertAlign w:val="superscript"/></w:rPr></w:style>
<w:style w:type="character" w:styleId="Hyperlink"><w:name w:val="Hyperlink"/><w:rPr><w:color w:val="0563C1"/><w:u w:val="single"/></w:rPr></w:style>
<w:style w:type="table" w:styleId="TableGrid"><w:name w:val="Table Grid"/><w:pPr><w:spacing w:after="0"/></w:pPr><w:tblPr><w:tblBorders><w:top w:val="single" w:sz="4" w:color="auto"/><w:left w:val="single" w:sz="4" w:color="auto"/><w:bottom w:val="single" w:sz="4" w:color="auto"/><w:right w:val="single" w:sz="4" w:color="auto"/><w:insideH w:val="single" w:sz="4" w:color="auto"/><w:insideV w:val="single" w:sz="4" w:color="auto"/></w:tblBorders><w:tblCellMar><w:left w:w="108" w:type="dxa"/><w:right w:w="108" w:type="dxa"/></w:tblCellMar></w:tblPr></w:style>
</w:styles>'''

numbering = f'''{DECL}<w:numbering {W}>
<w:abstractNum w:abstractNumId="0"><w:multiLevelType w:val="hybridMultilevel"/>
<w:lvl w:ilvl="0"><w:start w:val="1"/><w:numFmt w:val="bullet"/><w:lvlText w:val="•"/><w:lvlJc w:val="left"/><w:pPr><w:ind w:left="720" w:hanging="360"/></w:pPr><w:rPr><w:rFonts w:ascii="Symbol" w:hAnsi="Symbol" w:hint="default"/></w:rPr></w:lvl>
</w:abstractNum><w:num w:numId="1"><w:abstractNumId w:val="0"/></w:num></w:numbering>'''

settings = f'{DECL}<w:settings {W}><w:proofState w:spelling="clean" w:grammar="clean"/><w:defaultTabStop w:val="720"/><w:characterSpacingControl w:val="doNotCompress"/></w:settings>'

content_types = f'''{DECL}<Types xmlns="http://schemas.openxmlformats.org/package/2006/content-types">
<Default Extension="rels" ContentType="application/vnd.openxmlformats-package.relationships+xml"/>
<Default Extension="xml" ContentType="application/xml"/><Default Extension="png" ContentType="image/png"/>
<Override PartName="/word/document.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"/>
<Override PartName="/word/styles.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.styles+xml"/>
<Override PartName="/word/settings.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.settings+xml"/>
<Override PartName="/word/numbering.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.numbering+xml"/>
<Override PartName="/word/footnotes.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footnotes+xml"/>
<Override PartName="/word/header1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.header+xml"/>
<Override PartName="/word/footer1.xml" ContentType="application/vnd.openxmlformats-officedocument.wordprocessingml.footer+xml"/>
</Types>'''
root_rels = f'{DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships"><Relationship Id="rId1" Type="http://schemas.openxmlformats.org/officeDocument/2006/relationships/officeDocument" Target="word/document.xml"/></Relationships>'
REL = 'http://schemas.openxmlformats.org/officeDocument/2006/relationships/'
doc_rels = (f'{DECL}<Relationships xmlns="http://schemas.openxmlformats.org/package/2006/relationships">'
            f'<Relationship Id="rIdStyles" Type="{REL}styles" Target="styles.xml"/>'
            f'<Relationship Id="rIdSettings" Type="{REL}settings" Target="settings.xml"/>'
            f'<Relationship Id="rIdNum" Type="{REL}numbering" Target="numbering.xml"/>'
            f'<Relationship Id="rIdFn" Type="{REL}footnotes" Target="footnotes.xml"/>'
            f'<Relationship Id="rIdHdr" Type="{REL}header" Target="header1.xml"/>'
            f'<Relationship Id="rIdFtr" Type="{REL}footer" Target="footer1.xml"/>'
            f'<Relationship Id="rIdImg" Type="{REL}image" Target="media/chart.png"/>'
            f'<Relationship Id="rIdLink" Type="{REL}hyperlink" Target="https://example.org/" TargetMode="External"/>'
            '</Relationships>')

out = os.path.join(os.path.dirname(__file__), 'fixtures', 'sample.docx')
os.makedirs(os.path.dirname(out), exist_ok=True)
with zipfile.ZipFile(out, 'w', zipfile.ZIP_DEFLATED) as z:
    z.writestr('[Content_Types].xml', content_types)
    z.writestr('_rels/.rels', root_rels)
    z.writestr('word/_rels/document.xml.rels', doc_rels)
    z.writestr('word/document.xml', document)
    z.writestr('word/styles.xml', styles)
    z.writestr('word/settings.xml', settings)
    z.writestr('word/numbering.xml', numbering)
    z.writestr('word/footnotes.xml', footnotes)
    z.writestr('word/header1.xml', header)
    z.writestr('word/footer1.xml', footer)
    z.writestr('word/media/chart.png', png(120, 60))
print(out)

# long.docx: never paginated by Word — no breaks of any kind — with a table
# taller than a page, to exercise Babel's own block-level pagination.
lbody = [p(r('Long document without page breaks'), ppr='<w:pStyle w:val="Heading1"/>')]
for i in range(40):
    lbody.append(p(r(f'{i + 1}. ' + LOREM * 2)))
ltbl = '<w:tbl><w:tblPr><w:tblStyle w:val="TableGrid"/><w:tblW w:w="9000" w:type="dxa"/></w:tblPr><w:tblGrid><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/><w:gridCol w:w="3000"/></w:tblGrid>'
for i in range(60):
    ltbl += '<w:tr>' + ''.join(cell(f'Row {i + 1} col {c + 1}') for c in range(3)) + '</w:tr>'
lbody.append(ltbl + '</w:tbl>')
for i in range(40):
    lbody.append(p(r(f'After table {i + 1}. ' + LOREM)))
for i in range(5):
    lbody.append(p(r(f'List item {i + 1}'), ppr='<w:pStyle w:val="ListParagraph"/><w:numPr><w:ilvl w:val="0"/><w:numId w:val="1"/></w:numPr>'))
ldoc = f'{DECL}<w:document {W}><w:body>{"".join(lbody)}{sect}</w:body></w:document>'
out2 = os.path.join(os.path.dirname(__file__), 'fixtures', 'long.docx')
with zipfile.ZipFile(out, 'r') as src, zipfile.ZipFile(out2, 'w', zipfile.ZIP_DEFLATED) as z:
    for n in src.namelist():
        z.writestr(n, ldoc if n == 'word/document.xml' else src.read(n))
print(out2)
