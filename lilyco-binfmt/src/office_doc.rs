//! `lbin office-doc` — Word 文档的结构：段落之外，这份文档还有什么。
//!
//! 常见需求不是「再读一遍正文」，而是这些问题：这篇文档有几段、几级标题、几张表、
//! 表里多少格、有没有图片与超链接、链接是不是都还在包内、脚注尾注批注有多少、
//! 分页分了几节、用了哪些样式、有没有修订与高亮。这些数在 WordprocessingML 里
//! 都是**数得出来的元素**，不依赖任何解释器的主观判断 —— 所以每个数都写清了是谁数的
//! （`word/document.xml` 里的元素种类），而不是报一个来源不明的「复杂度分数」。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::{open, relationships, Family};
use crate::read::read_blob;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};

/// 报出 Word 文档的结构（T0 只读）
#[derive(App)]
#[app(
    name = "office-doc",
    run = "run_office_doc",
    about = "Report the structure of a Word document: paragraph count (empty ones counted separately, because Word's own statistics do), headings with their level and text, every style used and how often, tables with rows and cells, inline shapes and pictures, hyperlinks split into internal and external with their targets, sections, explicit page/column breaks, footnotes and endnotes and comments (read from their own parts when present), tracked-change presence (w:ins / w:del counts) plus a revision ledger (revisions): one entry per logical change with its kind, author, date, the paragraph index it sits in and the words it carries - elements are merged only when adjacent with the same kind/author/date/paragraph, because a producer writes one edit as several runs (LibreOffice splits the number from the unit into two w:ins), while the ODF export of the very same file states them as one changed-region, which is what this merge rule was measured against. Paragraph-mark insertions (w:pPr/w:rPr/w:ins) are counted apart from the paragraph's text and are not merged with it; ODF keeps deleted words inside the region and inserted words between text:change-start and text:change-end in the body, and both are read. Legacy .doc reports revisions as null rather than guess (the redline tables live in the table stream, not the piece table). protection (docx: w:documentProtection in word/settings.xml - w:edit says what kind of editing is restricted and w:enforcement says whether it is on; odt: the ProtectForm/ProtectBookmarks/ProtectFields config-items in settings.xml, which is a different place and does NOT carry the docx restriction across - the same document converted to .odt reports false for all three, measured); numbering usage, headers and footers, embedded objects and custom XML, plus which optional parts the package actually carries. statistics answers 'how many words/pages': ours (characters, characters_no_spaces and words_by_space - the last split on whitespace only, which is why it is named that way and not 'words') next to the producer's own numbers (docx docProps/app.xml, ODF meta.xml document-statistic) because the two disagree by design - python-docx writes app.xml with Words/Characters at 0 (it never counted), and LibreOffice counts Chinese words rather than whitespace runs, while on the same text our character counts match its character-count exactly. For legacy .doc it falls back to what the piece table can honestly tell: paragraph count from the CP total plus the marker characters it dropped (cell ends, field boundaries), not a claim about tables it cannot see. For .odt it reports the same account from content.xml's office:text (headings from text:outline-level, comments from text:annotation, pictures from draw:image, footnotes and endnotes split out of the single text:note element by its note:class) and additionally echoes meta.xml's own document-statistic so the producer's numbers are visible next to ours. `contents` answers 'is there a table of contents and how many levels does it pull in', reported per family because the spellings share almost nothing: OOXML wraps a w:sdt whose docPartGallery reads Table of Contents (Word and LibreOffice both write it) and keeps the levels INSIDE the field instruction text - a form like TOC \\o \"1-2\" \\h, with the producer's own quoting - while the wrapper can also be absent and only the field present, so both are looked for; ODF keeps a text:table-of-content block whose name is on text:name and whose level is the source element's outline-level attribute, and LibreOffice additionally writes all ten entry templates whether or not they are used (entry_templates reports what is written, not what is used); RTF has no wrapper at all, only a field group whose instruction spells each switch with a DOUBLED backslash (a single one would open a control word instead), and the stream reader decodes that pair - which is why the levels string coming back from toc.rtf is byte-identical to the one from toc.docx and one switch reader serves both families, while the OOXML-only gallery and sdt keys stay absent rather than invented. A file without one reports present false - false, not missing; legacy .doc reports null because this reader does not look there. RTF is not a package but one stream, so that branch answers with only what the stream itself proves: structure.paragraphs is the lines the par control word cuts, footnotes and endnotes are counted apart from their destination groups (an endnote is a footnote group that additionally carries ftnalt), and pictures / embedded_objects / skipped_destinations / note_destinations / page_destinations come from the same walk, while sections, revisions and protection stay null, comments is a count of the annotation groups the stream holds (author and words come back from office-text, where an undecodable atndate stays null and the raw digits are handed over as written), and the break words are counted at word boundaries into structure.break_words - a substring count would be wrong twice over, because \\pard contains \\par and \\sectd contains \\sect, and a Word page break reaches this family as \\pagebb rather than \\page (page_breaks sums the three spellings; \\sect is a terminator the last section never writes, so section_breaks is reported while sections stays null) - null means 'this reader did not look', 0 would mean 'there are none'. Styles and fonts are read out of the fonttbl and stylesheet groups with a lookahead: those groups are still skipped as far as the body is concerned (so skipped_destinations did not move when this was added), but their entries come back as styles (name to how many paragraphs use it, taken from the body's own \\sN) plus style_definitions / font_definitions counts and a font_list; a font name written in a non-ANSI charset with non-ASCII bytes comes back as name null with that charset number, because decoding those as cp1252 would be a made-up name. Tables are the interesting middle case there: table_rows and table_cells ARE reported (they are simply how many times the row and cell control words appear, and on two measured files those counts match the same document's docx and odt ledgers exactly), while tables itself stays null because the rule for grouping rows into separate tables was tried against one single-table file and one two-table file and counted two as one. Links come from a lookahead into the field group (the HYPERLINK address inside the instruction, plus the display text of the result group - which stays in the body, because that is what the page shows), and fields counts how many field groups the stream holds since page numbers and dates are fields too but are not links. Headings on this branch are read from the style names: the \\sN in a paragraph's own properties is looked up in the stylesheet group and only an entry named `heading N` (case-insensitive, whitespace required between the word and the number, nothing after it) makes that paragraph a heading at level N - a custom style name yields no heading rather than a guessed one, and the character-style namespace is not consulted because \\csN numbering is separate. `page_setup` answers 'how big is the paper and what are the margins', per family because the three spellings share nothing but the number: OOXML writes twips (1/1440 inch) on each w:sectPr's w:pgSz and w:pgMar (a two-section document writes two entries, one per section), ODF writes self-describing lengths (fo:page-width=\"21.59cm\") on the page-layout in styles.xml - NOT content.xml, and the page-layout-properties entry that carries only grid settings is not a sheet of paper so it is neither reported nor numbered, and RTF writes a document-level \\paperw / \\paperh / \\marg* string (a per-section override would sit in the {\\*\\sectx} group, which this branch does not judge, so RTF gives exactly one entry). All of them convert to integer hundredths of a millimetre with one shared integer formula (round half up, no floating point, so the second reader cannot disagree in the last digit), and every entry also carries `written` - what the file literally says - because producers differ across families on the very same words: on notes-hf the docx writes 1440 twips top and bottom while LibreOffice's own odt and rtf exports both write 720 / 1.27cm, and this reports the three numbers instead of picking a winner. Orientation is only ever what the file wrote: OOXML and RTF omit it for portrait, so those come back null while ODF says portrait. Legacy .doc reports null because its section properties live in the table stream, which this reader does not walk. structure.paragraph_formats answers 'what did THIS paragraph say about how it is laid out', and per family because the two families do not put the answer in the same place: OOXML writes it on the paragraph's own w:pPr (w:jc for alignment, w:ind and w:spacing as attribute tables, the style named by w:pStyle), so entries carry index, style, elements (the child names the paragraph actually wrote), alignment, indent, spacing and chars_written - a second unit lives beside the twips on w:ind (w:leftChars=\"200\" means two character widths, and this reports the string it wrote rather than converting it); ODF writes nothing on the paragraph but a style name (text:p/@text:style-name) and keeps the properties one hop away on that style's style:paragraph-properties (fo:text-align, fo:margin-*, fo:line-height - which may be a percentage or an absolute length, and a hanging indent arrives as a NEGATIVE fo:text-indent), so an entry says resolved true/false and, when the style is not the one this reader can see (a paragraph styled Standard lives in styles.xml, not content.xml), written stays null instead of borrowing a neighbour's numbers. Alignment values come back as written - OOXML says both/right/center, ODF says justify/start/end - because 'both' and 'justify' are a producer difference to show, not to normalize. structure.columns answers 'is this set in more than one column', again per family: OOXML writes one w:cols per w:sectPr, and num is only written by a producer that chose to write it, so sections / written / multi are counted apart from each other (a one-column section still has a w:cols); ODF wraps the columns in a text:section whose family=section style carries style:columns (fo:column-count, fo:column-gap) plus one style:column per column with a relative width (style:rel-width=\"32767*\", and two columns measured at 32767* and 32768* - the halves come from the producer's rounding, not from ours), and style:dont-balance-text-columns is echoed under the key dont_balance because the value \"true\" means the opposite of what a key called 'balance' would promise. RTF and legacy .doc write neither key at all: an absent key means 'this reader did not look in this family', which is not the same claim as 0 or null - LibreOffice's RTF export re-emits the style defaults on every paragraph (so a paragraph ledger there would list the same numbers on every line and prove nothing) and no \\cols control word is written to look for. structure.run_formats is a second book beside paragraph_formats - 'what did THESE characters say about themselves' - because the two live in different places: OOXML writes character formatting on the run's own w:rPr as CHILD ELEMENTS (w:b / w:i / w:strike / w:u are the four switches; an element that writes no w:val means on, while w:val=0 means off, so 'has a b child' is not the same claim as 'is bold' and the reading is published as a tri-state), so each run carries props_written (does the rPr element exist at all), props_attrs, elements, switches, the named values (color, highlight, size still in half-points, position, and the whole rFonts table because that element spells one choice across four attributes) and format - every child with its attributes as written, in file order. The two producers answer the existence question in OPPOSITE ways on this very file: python-docx writes no rPr for an unformatted run while LibreOffice's rewrite adds an empty rPr to every single one of them, so with_props and props_empty are counted apart (14/0 in one file, 30/16 in the other, both with_format 14 - folding those two into one boolean would turn a producer habit into a statement about the document). Off is spelled four ways across the four files measured here: 0, false, fo:font-weight normal, and a b0 whose negation is a digit glued to the control word. ODF writes neither an element nor an attribute on the run: a text:span names a family=text style and the properties sit one hop away on its style:text-properties, so entries carry style / resolved / found_in (content.xml first, styles.xml second, because automatic text styles and named character styles live in different parts) / parent (reported, not followed) / written (attribute names kept with their prefixes) and switches read through this family's own vocabulary. Measured third case: characters between spans are not a run at all here - they are the paragraph's own text - so those rows carry element #text with style AND resolved null, which is 'there was no number to look up', not 'a lookup failed'. RTF has no run element either: the words ride on the group header (b, i, strike, super, cf23, highlight7, fs18, af9), an underline for CJK text arrived in the Asian pocket so underline_word names which of the four pockets the file chose while the switch is read once across them, and the numbers hop through the file's own tables (colortbl for cf / cb / highlight - which is how a bare 23 becomes C00000 - and fonttbl for f / af, where a name this reader cannot decode stays null even though the entry was found). Because this family never wrote a per-run element to look inside, with_props and props_empty are null rather than 0, and control words said outside every group (LibreOffice re-issues the style defaults on each paragraph) are counted apart in words_outside_groups. A run can also say nothing itself and just name a CHARACTER STYLE: OOXML writes w:rStyle inside the same rPr, and the sentence lives in a w:type=character entry of word/styles.xml, so each run carries style / style_found / style_name / style_parent / style_format (that definition's own rPr children, with the rStyle link itself filtered out) and style_switches read through the SAME four switches - while switches keeps reporting only what the run wrote itself. Measured: the template's Strong says bold and the run says nothing, so bold_from_style counts 1 while bold_on counts 0 there; a run that names Emphasis AND writes w:b has the two places saying different halves, which is why where_both_spoke is published separately rather than merged. The id and the name are not the same string (styleId SubtleEmphasis is named 'Subtle Emphasis'), the parent chain is reported but never followed, and a theme colour written as w:themeColor/w:themeTint stays exactly that because resolving it needs theme1.xml, which this ledger does not open. ODF has no such split at all: the span's style IS where the properties live, so its switches are the style's by construction and the two families' tallies are deliberately not comparable (bold_on is 2 on the .odt of the same text and 1 on the .docx - one counted the style, the other counted the run). Span styles nest (an outer Emphasis around an inner T1, measured), so rows carry depth and a span's own text counts only the characters it holds directly - otherwise one sentence is reported twice. RTF names a character style with \\csN and its stylesheet entry is written as {\\*\\cs34 … Strong;} - a group whose whole point is 'skip me if you do not know me', which is exactly why the name used to come back null: the definition reader now strips that marker before extracting the name (the footnote lesson again: knowing a group is not a licence to lose its contents), so character_style resolves id 34 to the name Strong while the same group header ALSO carries the style's own flattened \\b - both places stay visible. Legacy .doc writes none of these keys. Both readers drop namespace declarations from every attribute table (xmlns= and xmlns:fo= say how to read a name, not a value the file set) and the ODF tables keep their prefixes on purpose, because fo:text-indent and loext:text-indent are two different properties that a local-name-only merge would collide. The ODF ledger walks the SAME paragraph list as structure.paragraphs, which matters because a note sits INSIDE the paragraph that carries it (text:p > text:note > text:note-body > text:p): on notes-end.odt 'how many paragraphs' has two honest answers - 4 if the walk stops at each text:p, 7 if every text:p in the tree counts - and LibreOffice's own meta.xml paragraph-count is the 7, so the format ledger reports the 4 (indexes would be meaningless against a different list) while the producer's 7 stays visible next to it in statistics.producer. structure.numbering answers 'is this paragraph a list item, at what level, and where did that come from', and it has to look in three places because OOXML lets a producer put the same choice in any of them: the paragraph's own w:pPr/w:numPr, the w:pPr of the style the paragraph names (Word's List Number works that way and python-docx copies the habit - on lists.docx three of the six listed paragraphs say nothing themselves), or both at once, which is what LibreOffice's own docx export writes (it puts numId on the paragraph AND keeps it on the style, so from reads both). Resolving it is a three-hop (w:num/@numId -> its w:abstractNumId -> w:abstractNum -> the w:lvl whose @w:ilvl matches) and the two books of numbers are numbered separately - measured numId 1 -> abstractNumId 8 and numId 5 -> 7 - so no id is ever taken as the other. Each paragraph comes back with which place it came from, both ids as written, the abstract it resolves to, three separate booleans for the three hops (resolved / abstract_found / level_found) and that level's own ledger: numFmt, lvlText, start and lvlJc as written plus the level's w:pPr/w:ind and w:rPr/w:rFonts attribute tables - because a bullet's lvlText is not U+2022 but U+F0B7 in the Symbol font, and without the font name that character is an unreadable box. Nothing is filled in when the file did not point at a level: the template's abstracts are multiLevelType=singleLevel with one w:lvl, so a paragraph naming ilvl=1 gets level_found false rather than level 0's numbers, and a paragraph naming a numId that is not in numbering.xml gets resolved false - the same document rewritten by LibreOffice turns that dangling 77 into numId 0, which is still no number that exists, so two producers point at nothing in two different ways and both are reported as written. definitions lists every w:num with the abstract it names, whether any paragraph pointed at it (the template ships nine, this document uses four) and that abstract's levels. The per-cell ledger rides on the same table walk (shading, borders, vertical align): `w:shd` and `w:vAlign` are attribute maps as written, `w:tcBorders` is reported twice on purpose - `borders_present` says the element exists while `borders` lists only the edges that actually wrote something, because LibreOffice's rewrite of this file emits an EMPTY <w:tcBorders></w:tcBorders> on all six cells (five cells therefore read present=true with no edges, while python-docx's untouched cells read present=false), and collapsing those two would turn a producer habit into a claim about the document. Cell values themselves survived the rewrite unchanged, including the hex case FFFF00 / FF0000 and the sz=6 double border; only attribute order moved. For .odt the same question has another shape: the level is NESTING (text:list > text:list-item > ..., depth counted by this reader, and the inner text:list frequently writes no style-name at all - chain reports what each level did write), the pointer again exists in two places (the paragraph's style carries text:list-style-name while the list element carries text:style-name), levels of a list style are written from 1 (text:level) where OOXML writes them from 0 (w:ilvl) so neither is converted, and LibreOffice's export puts every text:list-style definition in styles.xml while the paragraph styles sit in content.xml - so each entry says which part resolved it (style_part / list_part) and in_content / in_styles count the two piles. A paragraph whose style says text:list-style-name=\"\" comes back with that empty string and resolved false: this family spells 'no list' by writing an empty name, which is not the same claim as a name pointing at a missing definition. RTF now reports the same question with its own shape: the paragraph says \\ilvl and \\ls (and beside them a second \\li / \\fi pair, which is the paragraph's own indent, NOT the level's - both are given side by side because the file holds both), the number lands first in the listoverride table (`{\\listoverride\\listid4\\listoverridecount0\\ls4}`, where listoverridecount says how many levels that override rewrites - measured 0, i.e. it only re-points the number), the override names a \\listid, and that definition is a `{\\list\\listtemplateidN ...}` group whose own \\listid is written at the END of the group, which is why grabbing `{\\list\\listid` finds nothing while the file holds 14 occurrences of \\listid, seven in each table. Levels carry no \\ilvl from this producer at all, so the level number is which `{\\listlevel` this is in order, and every level is written whether used or not (7 definitions x 9 levels = 63 levels), each keeping levelnfc / leveljc / levelstartat / levelfollow plus `{\\leveltext ...}` and `{\\levelnumbers ...}` verbatim - the placeholder notation \\'02\\'00.; is reported as written, not decoded into a format string. This family additionally writes the producer's OWN computed label into the stream (`{\\listtext\\pard\\plain  1.\\tab}`), so an entry carries label (decoded), label_written (that literal group, leading space and all), label_tab and label_font: on the bullet the level points at font 1 (Symbol in the font table) while the rendered label says \\f7, and both numbers are reported instead of one being picked. `{\\*\\listtable` is starred while \\listoverridetable in the same file is not, so the lookahead has to hang off both paths, and skipped_destinations did not move when this was added. Legacy .doc still reports no numbering key: that one keeps lists in the table stream. structure.table_layouts answers 'how wide did this table say it is', and OOXML keeps three books that need not agree: w:tblPr/w:tblW is the table's own claim (python-docx writes type=auto w=0, a value that says nothing, while LibreOffice's rewrite of the same file computes 8640 dxa and additionally writes w:jc, w:tblInd, w:tblLayout and an EMPTY w:tblCellMar), w:tblGrid/w:gridCol@w is the grid, and every cell carries its own w:tcPr/w:tcW - on the merged fixture the grid says three columns of 2880 while the horizontally merged cell says 5760 (the sum of the two it covers), so 'how wide' is a different number per book and none is picked for the reader; rows and cells count only this table's direct children, so a table nested in a cell does not inflate the book above it. The twips are converted with the same integer formula the page-size ledger uses, which is what makes the cross-family comparison honest: python-docx's 4320 twips, LibreOffice's 7.62cm and the 15.24cm table width land on 7620 and 15240 in both readers. For .odt there is no table-level width element at all: one table:table-column can stand for several columns (number-columns-repeated - the fixture writes two columns as ONE element with repeated=2, so 'how many column elements' and 'how many columns' are counted apart), the width sits one hop away on a style:style of family=table-column under style:table-column-properties/@style:column-width, and - unlike the list styles of the numbering ledger, which LibreOffice puts in styles.xml - those column styles and the family=table style carrying style:width are written in content.xml, so every entry names the part it resolved from (style_part). The cell ledger on this family has to take one more hop, because a cell writes nothing about itself but a name: table:style-name points at an automatic style (LibreOffice spells the name from the address, 表格1.A1) whose style:table-cell-properties hold the shading as fo:background-color (lowercase, with a leading hash, where OOXML wrote w:fill FFFF00 uppercase without one), the borders as fo:border plus one fo:border-<edge> per side (each value a whole triple of width, style and colour - a double border arrives as 2.25pt for the three lines together while a sibling style:border-line-width-top lists each line at 0.026cm, which is the same w:sz 6 said twice, and since the shorthand form is also a border key, lined is false whenever every value it wrote says none), and the vertical align as style:vertical-align. padded reports that the style wrote an fo:padding-* at all (LibreOffice emits those defaults on every one of the six cells), style_part names the part the cell style resolved from and cell_styles_in_content / cell_styles_in_styles count the two piles, while cells_unresolved counts cells that named a style nobody defined. Covered placeholders are a second account: cell_elements counts table-cell together with table:covered-table-cell and covered_cells counts only the placeholders, because on the merged fixture the horizontal one writes no attribute at all while the vertical one still names a style - and the span itself stays on the cell as number-columns-spanned / number-rows-spanned rather than being folded into the per-row column count. RTF and legacy .doc report neither key: the first has no table width in this file family (only \\intbl and cell separators), the second keeps it in the table stream. `picture_list` answers 'what pictures does this document hold, and what did each one say about itself', and the same statement is kept in TWO PLACES on purpose because the producers put it in two places: size sits on `wp:extent` AND on `pic:spPr/a:xfrm/a:ext` - measured, both written, and NOT the same number (python-docx 1440000 = 4000, LibreOffice 1440180 = 4001, and the rewrite moves both), so neither is picked for the other; alt text sits on `wp:docPr` AND on `pic:cNvPr` (one producer writes descr only outside while the inner name is still the original FILE NAME `dot.png`, the rewrite copies both sentences inward); locks likewise (`a:graphicFrameLocks` vs `a:picLocks` - one file writes one, the rewrite writes both). Folding any of these into 'has alt text' / 'aspect locked' would read a producer habit as a statement about the document, so each place gets its own key plus `descr_written` to keep an EMPTY descr apart from never written. The blip names an id only (`a:blip/@r:embed`) and ids are each producer's own numbering (the same part is rId9 in one file and rId2 in the other), so the id and the resolved member path are both reported - and the lookup is restricted to THIS part's own relationship table, because a package can legally write rId2 again in `word/header1.xml.rels`. Floating placement is a third shape (`wp:anchor`, measured on a LibreOffice export of an ODT whose anchor was set to page, since no producer here writes one on its own): placement is in the ELEMENT NAME, wrapping in an element name plus its attributes (`wp:wrapSquare wrapText=largest`), and position is half attributes half text - `relativeFrom` on `wp:positionH`, the value inside the child (`<wp:align>center` is a word, `<wp:posOffset>635` is an EMU number), so each of position_h / position_v reports attributes, element name and text. ODF spells the same picture differently: sizes are self-describing length strings on the frame (`svg:width=4.001cm` - the same 4001 after the one shared integer formula), placement is an ATTRIBUTE (`text:anchor-type`) rather than an element name, the address is `draw:image/@xlink:href` with no relationship hop, and alt text moves from an attribute to a CHILD ELEMENT (`svg:desc`), which is why `alt_written` asks about the element. Measured surprise: the same choice is spelled two different ways in this family across a round trip (`as-char` in the export of the inline picture, `char` in the ODT re-exported from the anchored docx), and that round trip drops the OOXML anchor and wrapping entirely - both go out as written and are never reconciled. RTF is a third spelling and is now read per picture too, but with its own shape: size is written in THREE UNITS at once (\\picw/\\pich pixels, \\picwgoal/\\pichgoal twips, \\picscalex/\\picscaley percent) and the file states no DPI anywhere, so the pixel pair is handed over unconverted while the twip pair also gets the same integer 0.01mm reading the page ledger uses - and multiplying the three to recover what the page shows is an inference, so it is not reported (measured: images.docx says 1440000 EMU = 4000, while images.rtf says 480 twips = 847 with a 472% scale, and the RTF export of that very docx is the one that split it that way). There are TWO credentials for what format the bytes are: the control word before the data (\\pngblip) and the first eight bytes decoded out of the hex run itself; both are reported, and `sig_agrees` compares them only when both sides speak a name this reader knows (a \\dibitmap has no such stem, so it gets null rather than an invented mismatch). Alt text moved again: it lives in a `{\\*\\picprop}` shape-property table as a pair of groups (`{\\sn wzDescription}` names it, `{\\sv ...}` carries the value), so `props_written` says whether that table exists at all, `alt_written` whether the wzDescription pair does, and `alt` may be an EMPTY string - which is exactly what notes.rtf and toc.rtf do with the template logo, and why pictures_with_alt_text counts 0 there while alt_written is true: the file spoke, it said nothing. Hex data may be wrapped onto several lines, so the byte reader skips whitespace and stops at the first byte that is neither hex nor blank. The lookahead does not move the cursor or change skip, so the data still contributes no text and skipped_destinations did not move when this was added. A `w:drawing` that is neither inline nor anchor is not pushed as a placeholder entry either - the gap shows up as `drawings` minus `pictures`, and the legacy `w:pict` element is not read at all, so it is not counted here either. `tables[].grid` answers 'what is IN this table': rows of cells, each with its text (the cell's own paragraphs joined with newlines - a table nested inside a cell contributes nothing here), plus only the merge numbers the file actually wrote: OOXML spells a horizontal merge as w:gridSpan and OMITS the covered cell entirely (so a merged row has fewer w:tc than the table is wide), while ODF writes the covered cell as an empty table:covered-table-cell and spells the span as number-columns-spanned / number-rows-spanned / number-columns-repeated - on the measured 2x3 table with one horizontal merge the same visual row comes back as 2 cells in docx and 3 in odt, so cell counts are a property of the storage, not of the page, and rows / cells (counted with descendants, which is the 'how many row and cell markers does this file hold' question that also folds nested tables in) stay a separate account from grid.rows (direct children only). Legacy .doc reports null because its section properties live in the table stream, which this reader does not walk. Each run also answers 'what is inside it' rather than 'what does it say': contents lists every child except rPr in file order (t / tab / br / drawing / footnoteReference / instrText / fldChar, plus names this reader does not recognise) while text counts only the w:t characters - a field instruction run therefore carries the whole TOC switch string in instructions and an empty text, because those characters are not on the page - and the single id a note reference writes is resolved against TWO namespaces (footnote 2 and endnote 2 are two different notes: matching on the id alone sends both to part entry 0, measured on notes-end.docx where they sit at 0 and 2), so note reports kind, id, whether it resolved and which part entry, while notes_in_parts, notes_referenced and notes_unreferenced read that ledger from both ends. A commentReference hands over its id but does not hop into the note parts (comments live in their own part), which is why runs_with_ref can be 1 while ref_found is 0: that zero is 'counted, none', not 'did not look'. ODF rows for characters between spans keep all four switch keys as null, because a missing key means 'this reader did not look in this family', never 'it looked and nothing spoke'. A run can also sit inside a SHELL the paragraph writes around it: w:hyperlink, w:ins and w:del carry the author, the date, the revision id and the link id (r:id), none of which live on the run itself, so every row reports wrapped (the shell's element name or null) and wrapped_written (that shell's attributes exactly as written), tallied apart as runs_wrapped / wrapped_hyperlink / wrapped_ins / wrapped_del. Measured on one insertion: python-docx writes a single shell (id 11) around the whole phrase while LibreOffice's rewrite splits it into two shells (ids 0 and 1) and renumbers every id in the file - ids are therefore listed, never compared across producers. Deletion is a fifth case: the removed characters are written in w:delText rather than w:t, so that run's text is empty while contents still names delText and the author and date come from the shell, and those same characters stay visible in the revision ledger. Returns { path, format, kind, structure, page_setup, headings, styles, tables, images, hyperlinks, contents, revisions, protection, statistics, parts, notes }. Read-only (safety T0)."
)]
pub struct OfficeDoc {
    /// Word 文档（docx / docm / doc / odt / rtf）
    #[arg(about = "Word document to structure", must_exist = true)]
    path: PathBuf,

    /// 最多列多少个标题/链接（总数照实给）
    #[arg(
        about = "List at most this many items per collection",
        default = 100,
        min = 1
    )]
    limit: u64,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

/// CLI 的 `#[arg(default = N)]` 与各端省略参数时的回退值必须是同一个数
const LIMIT_DEFAULT: usize = 100;

/// 字数与字符数：口径写在键名上。`words_by_space` 就是「按空白切的词」——
/// 一整段中文可能只算一个「词」，那不是数错，是这个口径对中文意义有限，
/// 所以它必须与 `characters_no_spaces` 一起看（Word/LibreOffice 自己的「字数」
/// 是另一套规则，这里不模仿，只把它自报的那份照抄在下面）
#[derive(Default)]
struct Tally {
    characters: usize,
    no_space: usize,
    words_by_space: usize,
}

impl Tally {
    fn add(&mut self, text: &str) {
        self.characters += text.chars().count();
        self.no_space += text.chars().filter(|one| !one.is_whitespace()).count();
        self.words_by_space += text.split_whitespace().count();
    }

    fn to_json(&self) -> Value {
        json!({
            "characters": self.characters,
            "characters_no_spaces": self.no_space,
            "words_by_space": self.words_by_space,
        })
    }
}

/// `docProps/app.xml` 里生产者自报的那几个数。python-docx 写的样本里
/// `Words` / `Characters` / `Paragraphs` 全是 0 —— 那是「它没数过」，不是
/// 「这份文档没有字」，所以两份账并排放，谁也不许盖掉谁。
fn producer_counts(bytes: &[u8]) -> Value {
    let Ok(member) = zipread::member(bytes, "docProps/app.xml", DEFAULT_MEMBER_CAP) else {
        return Value::Null;
    };
    let root = xmlscan::parse_str(&member.as_text());
    let mut out = serde_json::Map::new();
    for (key, want) in [
        ("words", "Words"),
        ("characters", "Characters"),
        ("paragraphs", "Paragraphs"),
        ("lines", "Lines"),
        ("pages", "Pages"),
    ] {
        let found = root.descendants(want);
        let Some(one) = found.first() else {
            continue;
        };
        let raw = one.text().trim().to_string();
        out.insert(
            key.to_string(),
            match raw.parse::<i64>() {
                Ok(number) => json!(number),
                Err(_) => json!(raw),
            },
        );
    }
    if out.is_empty() {
        Value::Null
    } else {
        Value::Object(out)
    }
}

/// 域指令里 `\o "1-2"` 那一对引号之间的字（`&quot;` 在解析时已经还原成 `"`）
fn switch_value(instruct: &str, switch: &str) -> Option<String> {
    let at = instruct.find(switch)? + switch.len();
    let rest = &instruct[at..];
    let start = rest.find('"')? + 1;
    let stop = rest[start..].find('"')? + start;
    Some(rest[start..stop].to_string())
}

/// 一处尺寸（`wp:extent` 或 `pic:spPr/a:xfrm/a:ext`）：写的数与换算出来的 0.01mm
pub(crate) fn size_row(node: Option<&xmlscan::Node>) -> Value {
    let Some(one) = node else { return Value::Null };
    let mm = |want: &str| -> Value {
        one.attr_local(want)
            .and_then(|raw| crate::paper::emu(raw))
            .map(|yes| json!(yes))
            .unwrap_or(Value::Null)
    };
    json!({
        "cx": one.attr_local("cx").map(|raw| raw.to_string()),
        "cy": one.attr_local("cy").map(|raw| raw.to_string()),
        "mm_w": mm("cx"),
        "mm_h": mm("cy"),
    })
}

/// 一句替代文字（`wp:docPr` 或 `pic:cNvPr`）：`descr` 在不在单列一个键，
/// 因为「有没有替代文字」正是无障碍检查在问的事，而空的 `descr=""` 与没写也不是一回事
fn alt_row(node: Option<&xmlscan::Node>) -> Value {
    let Some(one) = node else { return Value::Null };
    json!({
        "id": one.attr_local("id").map(|raw| raw.to_string()),
        "name": one.attr_local("name").map(|raw| raw.to_string()),
        "descr": one.attr_local("descr").map(|raw| raw.to_string()),
        "descr_written": one.attr_local("descr").is_some(),
    })
}

/// 浮起来的那张图摆在哪里（`wp:positionH` / `wp:positionV` 各一条）：「相对什么」写在
/// 元素自己的属性上，「摆在哪」写在孩子的**名字与文字**里 —— `wp:align` 是一个词
/// （`center`），`wp:posOffset` 是一个 EMU 数（`635`），两种都不是属性，所以两处都交
fn position_row(node: Option<&xmlscan::Node>) -> Value {
    let Some(one) = node else { return Value::Null };
    let kid = one.children.iter().find(|one| one.local() != "#text");
    json!({
        "written": attr_map(one),
        "element": kid.map(|one| one.local().to_string()),
        "value": kid.map(|one| one.text()),
    })
}

/// OOXML 里的图：`w:drawing` 里面那一层才说明它怎么摆（`wp:inline` 坐在文字流里，
/// `wp:anchor` 是浮在页上 —— 后一种是 LibreOffice 从一份 `text:anchor-type="page"` 的
/// ODT 导出来的那份件量到的）。尺寸有**两处**（`wp:extent` 与 `pic:spPr/a:xfrm/a:ext`）：
/// 实测两家写的不是同一个数（`1440000` 与 `1440180`，换算 4000 与 4001），两处都交、
/// 不挑一个。字本身在一个部件里，格子上只留一个号（`a:blip/@r:embed`），
/// 地址在这份件的关系表里。
/// 替代文字有**两处**（`wp:docPr` 与 `pic:cNvPr`）：python-docx 只在外头那处写 `descr`，
/// LibreOffice 重写时里头那处也抄了一份；锁也是**两处**（`a:graphicFrameLocks` 与
/// `a:picLocks`），一家写一处、另一家两处都写 —— 各按各的交，不合成一个「有替代文字」。
/// 「两种摆法都不是」的那种 `w:drawing` 不进这本账：缺口由 `structure.drawings` 与
/// `structure.pictures` 的差自己说出来，不在这儿造一条没有件量过的形状。
fn docx_pictures(body: &xmlscan::Node, rels: &[crate::opack::Rel], limit: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for drawing in body.descendants("drawing").into_iter().take(limit) {
        let mut frames = drawing.descendants("inline");
        if frames.is_empty() {
            frames = drawing.descendants("anchor");
        }
        let Some(frame) = frames.into_iter().next() else {
            continue;
        };
        let blip = frame.descendants("blip").into_iter().next();
        let embed = blip.and_then(|one| one.attr_local("embed"));
        // 号只在**这份件自己的**关系表里查：同一个包里 `word/header1.xml.rels` 也敢再写一条 rId2
        let target = embed.and_then(|want| {
            rels.iter()
                .find(|one| one.id == want && one.source == "word/document.xml")
                .and_then(|one| one.resolved.clone())
        });
        let wrap = frame
            .children
            .iter()
            .find(|one| one.local().starts_with("wrap"));
        // `wp:extent` 是 frame 的直接孩子；退一步全树找只是为了「壳换了地方」那种件
        let mut extent = frame.child("extent");
        if extent.is_none() {
            extent = frame.descendants("extent").into_iter().next();
        }
        out.push(json!({
            "placed": frame.local(),
            "written": attr_map(frame),
            "effect_extent": frame.child("effectExtent").map(attr_map),
            "extent": size_row(extent),
            "pic_extent": size_row(frame.descendants("ext").into_iter().next()),
            "simple_pos": frame.child("simplePos").map(attr_map),
            "position_h": position_row(frame.child("positionH")),
            "position_v": position_row(frame.child("positionV")),
            "alt": alt_row(frame.descendants("docPr").into_iter().next()),
            "alt_in_picture": alt_row(frame.descendants("cNvPr").into_iter().next()),
            "blip_id": embed.map(|raw| raw.to_string()),
            "target": target,
            "locks": frame
                .descendants("graphicFrameLocks")
                .into_iter()
                .next()
                .map(attr_map),
            "pic_locks": frame.descendants("picLocks").into_iter().next().map(attr_map),
            "wrap": wrap.map(|one| one.local().to_string()),
            "wrap_written": wrap.map(attr_map),
            "graphic_children": frame
                .descendants("graphicData")
                .into_iter()
                .next()
                .map(|one| {
                    one.children
                        .iter()
                        .map(|kid| kid.local().to_string())
                        .collect::<Vec<String>>()
                })
                .unwrap_or_default(),
        }));
    }
    out
}

/// ODF 里的图是另一套：尺寸在 `draw:frame` 上写的是**自带单位的串**（`svg:width="4.001cm"`），
/// 摆法写在**属性** `text:anchor-type` 上（OOXML 写在元素名上），地址是
/// `draw:image/@xlink:href`（相对包内路径，没有关系表这一层），而替代文字搬到了
/// 孩子元素 `svg:desc` 里 —— 属性变元素，「写没写这个元素」因此要另问一句
/// （`alt_written`）。摆法这一族实测有两种拼法：同一段稿子从 docx 转出的是 `as-char`，
/// 从那份带 `wp:anchor` 的 docx 转回来成了 `char`，照文件交，不折成同一个词。
/// 属性表按文件写的名字交（`svg:` 与 `draw:` 留着），与 ODF 别处同一口径。
/// 只数带 `draw:image` 的那些 frame —— 装嵌入对象与图的是 `draw:object`，另有一份账。
pub(crate) fn odt_pictures(body: &xmlscan::Node, limit: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for frame in body
        .descendants("frame")
        .into_iter()
        .filter(|one| one.child("image").is_some())
        .take(limit)
    {
        let image = frame.child("image");
        let side = |want: &str| -> Option<i64> {
            frame
                .attr_local(want)
                .and_then(|raw| crate::paper::length(raw))
        };
        out.push(json!({
            "written": kept_attrs(frame),
            "placed": frame.attr_local("anchor-type").map(|raw| raw.to_string()),
            "style": frame.attr_local("style-name").map(|raw| raw.to_string()),
            "mm_w": side("width"),
            "mm_h": side("height"),
            "href": image
                .and_then(|one| crate::odsheet::attr_of(one, "href"))
                .map(|raw| raw.to_string()),
            "mime": image
                .and_then(|one| crate::odsheet::attr_of(one, "mime-type"))
                .map(|raw| raw.to_string()),
            "image_written": image.map(kept_attrs),
            "alt": frame.child("desc").map(|one| one.text()),
            "alt_written": frame.child("desc").is_some(),
        }));
    }
    out
}

/// 这份 docx 有没有目录、收了几级。OOXML 的目录有两种长相：
/// `w:sdt` 套着 `docPartGallery="Table of Contents"`（Word 与 LibreOffice 都这么写），
/// 或者一条 `TOC \o "1-2" \h` 的域指令（可以没有那个壳）。**「几级」写在域指令的文字里**，
/// 不是某个属性上 —— 与 ODF 那边是两种说法，所以两边各报各的，不强行统一
fn docx_contents(root: &xmlscan::Node) -> Value {
    let galleries: Vec<String> = root
        .descendants("docPartGallery")
        .iter()
        .filter_map(|one| one.attr_local("val").map(String::from))
        .collect();
    let mut instructions: Vec<String> = root
        .descendants("instrText")
        .iter()
        .map(|one| one.text().trim().to_string())
        .collect();
    for one in root.descendants("fldSimple").iter() {
        if let Some(had) = one.attr_local("instr") {
            instructions.push(had.trim().to_string());
        }
    }
    let fields: Vec<String> = instructions
        .into_iter()
        .filter(|one| one.to_uppercase().starts_with("TOC"))
        .collect();
    let gallery = galleries.iter().any(|one| one == "Table of Contents");
    json!({
        "present": gallery || !fields.is_empty(),
        "via": if gallery {
            json!("doc-part-gallery")
        } else if fields.is_empty() {
            Value::Null
        } else {
            json!("field")
        },
        "galleries": galleries,
        "fields": fields,
        "levels": fields.iter().find_map(|one| switch_value(one, r"\o ")),
        "sdt": root.descendants("sdt").len(),
    })
}

/// ODF 的目录：`text:table-of-content` 那一块。名字、受不受保护在元素属性上，
/// 「收几级」写在 `text:table-of-content-source` 的 `outline-level` 上 ——
/// 与 OOXML 把这一切塞进域指令文字正好是两种写法
fn odf_contents(root: &xmlscan::Node) -> Value {
    let blocks = root.descendants("table-of-content");
    if blocks.is_empty() {
        return json!({
            "present": false, "names": [], "outline_level": Value::Null,
            "entry_templates": 0, "title": Value::Null,
        });
    }
    json!({
        "present": true,
        "names": blocks
            .iter()
            .filter_map(|one| one.attr_local("name").map(String::from))
            .collect::<Vec<String>>(),
        "outline_level": root
            .descendants("table-of-content-source")
            .first()
            .and_then(|one| one.attr_local("outline-level"))
            .map(String::from),
        "entry_templates": root.descendants("table-of-content-entry-template").len(),
        "title": root
            .descendants("index-title-template")
            .first()
            .map(|one| one.text().trim().to_string())
            .filter(|had| !had.is_empty()),
    })
}

/// RTF 的目录：流里没有「目录」这种壳，只有一条自报家门的域 ——
/// `{\field{\*\fldinst { TOC \\o "1-2" \\h}}…}`。那一群的开关在文件里必须写成双反斜杠，
/// 解掉之后 `rtf.rs` 交回来的那一串与 docx 的 `w:instrText` **逐字同一个形状**，
/// 所以「几级」这把读取器两家共用（`galleries` / `sdt` 那两键 OOXML 专属，这里不造假）
fn rtf_contents(instructions: &[String]) -> Value {
    let fields: Vec<String> = instructions
        .iter()
        .filter(|one| one.to_uppercase().starts_with("TOC"))
        .cloned()
        .collect();
    json!({
        "present": !fields.is_empty(),
        "via": if fields.is_empty() {
            Value::Null
        } else {
            json!("field")
        },
        "fields": fields,
        "levels": fields.iter().find_map(|one| switch_value(one, r"\o ")),
    })
}

/// ODF 的换页不写在正文里：`text:p` 只带一个 `text:style-name`，而
/// `fo:break-before="page"` 坐在**那个样式自己的** `style:paragraph-properties` 上 ——
/// 与 .ods 的数据样式是同一类两跳。只看段落点名的那个样式：父样式链上也可能写，
/// 但手上四份件都写在自己身上，没有样本就不去猜那条链
fn odf_page_breaks(root: &xmlscan::Node, text_body: &xmlscan::Node) -> usize {
    let named: Vec<String> = root
        .descendants("style")
        .iter()
        .filter(|one| one.attr_local("family") == Some("paragraph"))
        .filter(|one| {
            one.child("paragraph-properties")
                .and_then(|had| had.attr_local("break-before"))
                == Some("page")
        })
        .filter_map(|one| one.attr_local("name").map(String::from))
        .collect();
    let mut hits = 0usize;
    for which in ["p", "h"] {
        for one in text_body.descendants(which) {
            if one
                .attr_local("style-name")
                .is_some_and(|had| named.iter().any(|want| want == had))
            {
                hits += 1;
            }
        }
    }
    hits
}

/// docx 的属性交**局部名**（`w:left` → `left`）：这一族的前缀不是契约的一部分。
/// 命名空间声明（`xmlns=` / `xmlns:前缀=`）不算属性 —— 标准库的 XML 读者也不把它放进 attrib
pub(crate) fn attr_map(node: &xmlscan::Node) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in &node.attrs {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        let local = key.rsplit(':').next().unwrap_or(key).to_string();
        out.insert(local, json!(value));
    }
    Value::Object(out)
}

/// ODF 的属性按**文件写的名字**交（前缀留着）：`fo:margin-left` 是 `3cm`，
/// `loext:margin-left` 是「两个字」—— 只按局部名收就会互相盖掉，那是替文件编东西。
/// 同上：`xmlns:` 那些是声明，不是属性
pub(crate) fn kept_attrs(node: &xmlscan::Node) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in &node.attrs {
        if key == "xmlns" || key.starts_with("xmlns:") {
            continue;
        }
        out.insert(key.clone(), json!(value));
    }
    Value::Object(out)
}

/// 从一份属性里挑出「局部名在这张表上」的那些，键仍按文件写的名字留着
fn pick_attrs(map: &serde_json::Map<String, Value>, want: &[&str]) -> Value {
    let mut out = serde_json::Map::new();
    for (key, value) in map {
        if want
            .iter()
            .any(|one| *one == key.rsplit(':').next().unwrap_or(key))
        {
            out.insert(key.clone(), value.clone());
        }
    }
    Value::Object(out)
}

/// ODF 段落格式里算「缩进」与「段距」的那两组属性名
const ODF_INDENT: [&str; 4] = [
    "margin-left",
    "margin-right",
    "text-indent",
    "auto-text-indent",
];
const ODF_SPACING: [&str; 4] = [
    "margin-top",
    "margin-bottom",
    "line-height",
    "contextual-spacing",
];

/// docx 的段落格式：`w:jc` / `w:ind` / `w:spacing` 就写在段自己的 `w:pPr` 上
///
/// 样式表里那份不算（那不是「这一段写的」），也不去解继承链；`w:leftChars="200"`
/// 是第二种单位（两个字），照字符串交，`chars_written` 只说「出现了 Chars 后缀」
fn docx_paragraph_formats(paragraphs: &[&xmlscan::Node], limit: usize) -> Value {
    let mut entries: Vec<Value> = Vec::new();
    let mut alignment = 0usize;
    let mut indents = 0usize;
    let mut spacings = 0usize;
    for (index, one) in paragraphs.iter().enumerate() {
        let Some(holder) = one.child("pPr") else {
            continue;
        };
        let elements: Vec<String> = holder
            .children
            .iter()
            .map(|kid| kid.local().to_string())
            .collect();
        let jc = holder.child("jc");
        let ind = holder.child("ind");
        let spacing = holder.child("spacing");
        let chars_written = ind
            .map(|kid| {
                kid.attrs
                    .iter()
                    .any(|(key, _)| key.rsplit(':').next().unwrap_or(key).ends_with("Chars"))
            })
            .unwrap_or(false);
        if jc.is_some() {
            alignment += 1;
        }
        if ind.is_some() {
            indents += 1;
        }
        if spacing.is_some() {
            spacings += 1;
        }
        entries.push(json!({
            "index": index,
            "style": holder.child("pStyle").and_then(|kid| kid.attr_local("val")).map(String::from),
            "elements": elements,
            "alignment": jc.and_then(|kid| kid.attr_local("val")).map(String::from),
            "indent": ind.map(attr_map).unwrap_or(Value::Null),
            "spacing": spacing.map(attr_map).unwrap_or(Value::Null),
            "chars_written": chars_written,
        }));
    }
    json!({
        "checked": paragraphs.len(),
        "listed": entries.len(),
        "with_alignment": alignment,
        "with_indent": indents,
        "with_spacing": spacings,
        "list": entries.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// docx 的分栏：一节一条 `sectPr/w:cols`（只有 `w:space` 没有 `w:num` 就是「一栏」的写法）
fn docx_columns(body: &xmlscan::Node, limit: usize) -> Value {
    let mut entries: Vec<Value> = Vec::new();
    for (at, sect) in body
        .descendants("sectPr")
        .into_iter()
        .take(limit)
        .enumerate()
    {
        let cols = sect.child("cols");
        entries.push(json!({
            "at": at,
            "present": cols.is_some(),
            "written": cols.map(attr_map).unwrap_or(Value::Null),
            "count": cols.and_then(|kid| kid.attr_local("num")).map(String::from),
            "space": cols.and_then(|kid| kid.attr_local("space")).map(String::from),
            "parts": cols
                .map(|kid| {
                    kid.children
                        .iter()
                        .filter(|had| had.local() == "column")
                        .map(attr_map)
                        .collect::<Vec<Value>>()
                })
                .unwrap_or_default(),
        }));
    }
    let written = entries
        .iter()
        .filter(|one| one["present"] == json!(true))
        .count();
    let multi = entries
        .iter()
        .filter(|one| {
            one["count"]
                .as_str()
                .and_then(|raw| raw.parse::<usize>().ok())
                > Some(1)
        })
        .count();
    json!({"sections": entries.len(), "written": written, "multi": multi, "list": entries})
}

/// ODF 的段落格式：段上只有一个样式名，属性在那个样式的 `style:paragraph-properties` 上
///
/// 一跳，只看段落自己点名的那个样式，而且**只找得到 content.xml 里那一份**：
/// 点名 `Standard` 的那一段的样式住在 styles.xml，那是文档默认不是这一段写的 ——
/// 所以那条交 `resolved: false`、`written: null`，父样式链更不去猜
fn odf_paragraph_formats(
    paragraphs: &[&xmlscan::Node],
    root: &xmlscan::Node,
    limit: usize,
) -> Value {
    let mut styles: Vec<(String, String, Option<String>, Option<Value>)> = Vec::new();
    for one in root.descendants("style").into_iter() {
        let family = one.attr_local("family").unwrap_or_default();
        if family != "paragraph" && family != "text" {
            continue;
        }
        let Some(name) = one.attr_local("name") else {
            continue;
        };
        styles.push((
            name.to_string(),
            family.to_string(),
            one.attr_local("parent-style-name").map(String::from),
            one.child("paragraph-properties").map(kept_attrs),
        ));
    }
    let mut entries: Vec<Value> = Vec::new();
    let mut resolved = 0usize;
    for (index, one) in paragraphs.iter().enumerate() {
        let name = one.attr_local("style-name");
        let found = name.and_then(|want| styles.iter().find(|one| one.0 == want));
        let written = found.and_then(|one| one.3.clone());
        let mut entry = json!({
            "index": index,
            "style": name.map(String::from),
            "resolved": found.is_some(),
            "family": found.map(|one| one.1.clone()),
            "parent": found.and_then(|one| one.2.clone()),
            "written": written.clone(),
        });
        if let Some(object) = written.as_ref().and_then(|had| had.as_object()) {
            entry["alignment"] = object.get("fo:text-align").cloned().unwrap_or(Value::Null);
            entry["indent"] = pick_attrs(object, &ODF_INDENT);
            entry["spacing"] = pick_attrs(object, &ODF_SPACING);
            resolved += 1;
        }
        entries.push(entry);
    }
    json!({
        "checked": paragraphs.len(),
        "listed": entries.len(),
        "resolved": resolved,
        "list": entries.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// OOXML 那四个「开关」：元素的**存在**本身不算表态（`<w:b w:val="0"/>` 说的是「不粗」），
/// 而表态有两家拼法：python-docx 写 `0`，LibreOffice 重写同一份件写 `false`
const RUN_SWITCHES: [(&str, &str); 4] = [
    ("b", "bold"),
    ("i", "italic"),
    ("strike", "strike"),
    ("u", "underline"),
];
/// 值类的那几个：元素名 → 交出去用的键名（值一律按文件写的原样，不换算单位）
/// 一串字里的「引用」那三种：元素名、它跳去的那本账（comment 不跳部件，只交号）、
/// 交出去用的键名
const RUN_REF_KINDS: [(&str, &str, &str); 3] = [
    ("footnoteReference", "footnote", "footnote"),
    ("endnoteReference", "endnote", "endnote"),
    ("commentReference", "comment", "comment"),
];
const RUN_VALUES: [(&str, &str); 5] = [
    ("color", "color"),
    ("highlight", "highlight"),
    ("sz", "size"),
    ("vertAlign", "position"),
    ("rFonts", "fonts"),
];
/// 「关」的四种写法（`w:u w:val="none"` 与 `<w:b w:val="off"/>` 都是文件里出现过的拼法）
const RUN_OFF: [&str; 4] = ["0", "false", "off", "none"];

/// 一串字里那四个开关的一个：没有这个孩子 = 没说（null）；有孩子而没写 `val` = 开
/// （OOXML 的默认，`<w:b/>` 就是粗体）；写了 `val` 才按那串字面判
fn run_switch(props: Option<&xmlscan::Node>, name: &str) -> Value {
    let Some(one) = props.and_then(|had| had.child(name)) else {
        return Value::Null;
    };
    match one.attr_local("val") {
        Some(raw) if RUN_OFF.contains(&raw) => json!(false),
        _ => json!(true),
    }
}

/// ODF 那四个开关（值住在一跳之外的样式里，拼法与 OOXML 完全不同）：
/// `normal` / `none` 是「明确写着不」，别的一切都是「写着要」，没有这一条才是「没说」
const ODF_RUN_SWITCHES: [(&str, &str); 4] = [
    ("fo:font-weight", "bold"),
    ("fo:font-style", "italic"),
    ("style:text-line-through-style", "strike"),
    ("style:text-underline-style", "underline"),
];
const ODF_RUN_OFF: [&str; 2] = ["normal", "none"];

fn odf_run_switch(written: Option<&Value>, name: &str) -> Value {
    let Some(raw) = written
        .and_then(|had| had.as_object())
        .and_then(|had| had.get(name))
        .and_then(|one| one.as_str())
    else {
        return Value::Null;
    };
    json!(!ODF_RUN_OFF.contains(&raw))
}

/// 段里的串：直接坐在段下的 `w:r`，加上超链接与修订那三个壳里的 `w:r`，
/// 每一条同时带上**包着它的那个壳**（没有壳交 null）。不往全树找，是为了不把
/// 文本框里另一段的字算到这一段头上；壳要跟着交，因为「这句话是插进来的」与
/// 「这句话是一条链接」都写在壳上，而作者、时间、`r:id`、`w:anchor` 这些字
/// 一串字自己一个字都没有
fn docx_runs(holder: &xmlscan::Node) -> Vec<(&xmlscan::Node, Option<&xmlscan::Node>)> {
    let mut out: Vec<(&xmlscan::Node, Option<&xmlscan::Node>)> = Vec::new();
    for one in &holder.children {
        match one.local() {
            "r" => out.push((one, None)),
            "hyperlink" | "ins" | "del" => {
                for had in one.children.iter().filter(|had| had.local() == "r") {
                    out.push((had, Some(one)));
                }
            }
            _ => {}
        }
    }
    out
}

/// `word/styles.xml` 里那些 `w:type="character"` 的定义：号、名字（可以与号不同）、
/// 父样式号与自己那份 `w:rPr` 的孩子。名字与父**只报不跟**（链是文件的，不是我们的）
fn docx_character_styles(
    styles: Option<&xmlscan::Node>,
) -> Vec<(String, Option<String>, Option<String>, Vec<Value>)> {
    let mut out: Vec<(String, Option<String>, Option<String>, Vec<Value>)> = Vec::new();
    let Some(tree) = styles else { return out };
    for one in tree.descendants("style").into_iter() {
        if one.attr_local("type") != Some("character") {
            continue;
        }
        let Some(id) = one.attr_local("styleId") else {
            continue;
        };
        let holder = one.child("rPr");
        let kids: Vec<&xmlscan::Node> = match holder {
            Some(had) => had
                .children
                .iter()
                .filter(|one| one.local() != "#text")
                .collect(),
            None => Vec::new(),
        };
        out.push((
            id.to_string(),
            one.child("name")
                .and_then(|kid| kid.attr_local("val"))
                .map(String::from),
            one.child("basedOn")
                .and_then(|kid| kid.attr_local("val"))
                .map(String::from),
            kids.iter()
                .filter(|one| one.local() != "rStyle")
                .map(|one| json!({"element": one.local(), "written": attr_map(one)}))
                .collect::<Vec<Value>>(),
        ));
    }
    out
}

/// 一串字的「字」只算 `w:t` 里的那些：域指令（`w:instrText`）不是页面上的字，
/// 图与引用更不是 —— `toc.docx` 里那句 `TOC \o "1-2" \h` 从头到尾没在页面上显示过，
/// 所以它交在 `instructions` 上而不是混进那串字的字里。只取**直接孩子**，不往全树找：
/// 一串字里的文本框（`w:drawing` 里的 `a:t`）是另一块地方写的字，有自己的账
fn run_text(run: &xmlscan::Node) -> String {
    run.children
        .iter()
        .filter(|one| one.local() == "t")
        .map(|one| one.text())
        .collect::<Vec<String>>()
        .join("")
}

/// 注那两份部件里的号：`(文件写的 id, 是哪一种)`。分隔符两条（`separator` /
/// `continuationSeparator`）不算注 —— 与 `structure.footnotes` 那本账同一个口径，
/// 两边数不一样就会在这里露出来
fn docx_note_index(bytes: &[u8]) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    for (part, kind) in [
        ("word/footnotes.xml", "footnote".to_string()),
        ("word/endnotes.xml", "endnote".to_string()),
    ]
    .into_iter()
    {
        let Some(member) = zipread::member(bytes, part, DEFAULT_MEMBER_CAP).ok() else {
            continue;
        };
        let tree = xmlscan::parse_str(&member.as_text());
        for one in tree.descendants(kind.as_str()).into_iter() {
            if matches!(
                one.attr_local("type").unwrap_or_default(),
                "separator" | "continuationSeparator"
            ) {
                continue;
            }
            if let Some(id) = one.attr_local("id") {
                out.push((id.to_string(), kind.clone()));
            }
        }
    }
    out
}

/// docx 的字符格式：段里的**每一串字**自己带一份 `w:rPr`，与段落格式（`w:pPr`）是两本账。
///
/// 三个数各交各的，因为两家生产者正好一边一种：python-docx 不给没格式的那一串写 `w:rPr`，
/// LibreOffice 重写时给**每一串**都补一个空的 `<w:rPr></w:rPr>` —— 所以 `with_props`
/// 与 `props_empty` 分开数（合成一个「有没有格式」就看不见这条差别）。
/// 格式住在 `rPr` 的**孩子元素**上（`<w:b/>`、`<w:color w:val="C00000"/>`），
/// `rPr` 自己一个属性也不写（`props_attrs` 因此是空表而不是 null）；
/// `format` 按文件写的顺序把每一对（元素名 + 自己的属性）原样交出去，
/// `switches` 与 `values` 只是从同一批孩子里按名字挑出来的那几格，没写的那格交 null。
/// 还有**第四个地方**：`w:rStyle` 只写一个号，那句话住在 `word/styles.xml` 那条
/// `w:type="character"` 的定义里（`style*` 那几键），所以「这串字是粗的吗」有两个来处 ——
/// 样式说了而段上没说的那几条各按开关数一本（`bold_from_style`…），
/// 两处都说了同一个开关的另数 `where_both_spoke`（不合成一个答案）。
///
/// 一串字里也可以**没有字**：`contents` 按文件写的顺序交出除 `rPr` 以外的每一个孩子
/// （`t` / `tab` / `br` / `drawing` / `footnoteReference` / `instrText` / `fldChar` …，
/// 连这份读者不认识的名字一起交），`text` 因此只算 `w:t` 里的那些字 ——
/// 域指令不是页面上的字。注的引用只有一个号（`refs`），号要跳到
/// `word/footnotes.xml` / `word/endnotes.xml` 才对得上（`note`：解开没有、是哪一种、
/// 排在部件第几条），而反向那一本 `notes_unreferenced` 数「部件里写着、正文没人引用」的注。
fn docx_run_formats(
    paragraphs: &[&xmlscan::Node],
    styles: Option<&xmlscan::Node>,
    bytes: &[u8],
    limit: usize,
) -> Value {
    let sheet = docx_character_styles(styles);
    let notes = docx_note_index(bytes);
    let mut entries: Vec<Value> = Vec::new();
    let mut checked = 0usize;
    let mut with_props = 0usize;
    let mut props_empty = 0usize;
    let mut with_format = 0usize;
    let mut with_style = 0usize;
    let mut style_found = 0usize;
    let mut where_both = 0usize;
    let mut with_text = 0usize;
    let mut with_ref = 0usize;
    let mut ref_found = 0usize;
    let mut field_runs = 0usize;
    let mut with_wrap = 0usize;
    let mut wrap_link = 0usize;
    let mut wrap_ins = 0usize;
    let mut wrap_del = 0usize;
    let mut on = [0usize; 4];
    let mut off = [0usize; 4];
    let mut from_style = [0usize; 4];
    let mut seen_refs: Vec<(String, String)> = Vec::new();
    for (para, holder) in paragraphs.iter().enumerate() {
        for (at, (run, wrap)) in docx_runs(holder).into_iter().enumerate() {
            checked += 1;
            let props = run.child("rPr");
            let kids: Vec<&xmlscan::Node> = match props {
                Some(had) => had
                    .children
                    .iter()
                    .filter(|one| one.local() != "#text")
                    .collect(),
                None => Vec::new(),
            };
            if props.is_some() {
                with_props += 1;
                if kids.is_empty() {
                    props_empty += 1;
                }
            }
            if !kids.is_empty() {
                with_format += 1;
            }
            let mut switches = serde_json::Map::new();
            for (which, pair) in RUN_SWITCHES.iter().enumerate() {
                let read = run_switch(props, pair.0);
                if read.as_bool() == Some(true) {
                    on[which] += 1;
                } else if read.as_bool() == Some(false) {
                    off[which] += 1;
                }
                switches.insert(pair.1.to_string(), read);
            }
            let mut values = serde_json::Map::new();
            for pair in RUN_VALUES.iter() {
                let found = props.and_then(|had| had.child(pair.0));
                let value = match found {
                    // `w:rFonts` 把名字写在**好几个属性**上（ascii / hAnsi / eastAsia / cs），
                    // 挑一个就是替文件决定它按哪个字体排，所以整张属性表交出去
                    Some(one) if pair.0 == "rFonts" => attr_map(one),
                    Some(one) => one
                        .attr_local("val")
                        .map(|raw| json!(raw))
                        .unwrap_or(Value::Null),
                    None => Value::Null,
                };
                values.insert(pair.1.to_string(), value);
            }
            // 样式那一跳：`w:rStyle` 只有一个号，那句话住在另一个部件里
            let named = props
                .and_then(|had| had.child("rStyle"))
                .and_then(|one| one.attr_local("val"))
                .map(String::from);
            let hit = named
                .as_ref()
                .and_then(|want| sheet.iter().find(|one| &one.0 == want));
            if named.is_some() {
                with_style += 1;
            }
            if hit.is_some() {
                style_found += 1;
            }
            let mut char_switches = serde_json::Map::new();
            for (which, pair) in RUN_SWITCHES.iter().enumerate() {
                let said =
                    hit.and_then(|one| one.3.iter().find(|had| had["element"] == json!(pair.0)));
                let value = match said {
                    None => Value::Null,
                    Some(had) => match had["written"]["val"].as_str() {
                        Some(raw) if RUN_OFF.contains(&raw) => json!(false),
                        _ => json!(true),
                    },
                };
                if value.as_bool().is_some() && switches[pair.1].as_bool().is_some() {
                    where_both += 1;
                }
                if value.as_bool().is_some() && switches[pair.1].is_null() {
                    from_style[which] += 1;
                }
                char_switches.insert(pair.1.to_string(), value);
            }
            // 这一串字里到底有什么
            let body: Vec<&xmlscan::Node> = run
                .children
                .iter()
                .filter(|one| one.local() != "#text" && one.local() != "rPr")
                .collect();
            let text = run_text(run);
            if !text.is_empty() {
                with_text += 1;
            }
            let mut refs = serde_json::Map::new();
            let mut note = Value::Null;
            for pair in RUN_REF_KINDS.iter() {
                let found = run
                    .child(pair.0)
                    .and_then(|one| one.attr_local("id"))
                    .map(String::from);
                // 批注的号也交，但它不跳注那两份部件（批注住在 comments.xml，另有一本账）
                if let (Some(id), true) = (found.clone(), pair.1 != "comment") {
                    seen_refs.push((id.clone(), pair.1.to_string()));
                    // 号是分种类的两条账：脚注的 `2` 与尾注的 `2` 是两条不同的注，只按号对会串门
                    let note_at = notes.iter().position(|one| one.0 == id && one.1 == pair.1);
                    if note_at.is_some() {
                        ref_found += 1;
                    }
                    note = json!({
                        "kind": pair.1,
                        "id": id,
                        "found": note_at.is_some(),
                        "at": note_at,
                    });
                }
                refs.insert(pair.2.to_string(), found.map(json).unwrap_or(Value::Null));
            }
            if refs.values().any(|one| !one.is_null()) {
                with_ref += 1;
            }
            let breaks: Vec<Value> = run
                .children
                .iter()
                .filter(|one| one.local() == "br")
                .map(|one| one.attr_local("type").map(json).unwrap_or(Value::Null))
                .collect();
            let instructions: Vec<Value> = run
                .children
                .iter()
                .filter(|one| one.local() == "instrText")
                .map(|one| json!(one.text()))
                .collect();
            let fields: Vec<Value> = run
                .children
                .iter()
                .filter(|one| one.local() == "fldChar")
                .filter_map(|one| one.attr_local("fldCharType").map(json))
                .collect();
            if !instructions.is_empty() || !fields.is_empty() {
                field_runs += 1;
            }
            // 这一串字是被谁包起来的：壳上有作者、时间、链接的号与锚，而串自己一个都没有
            let wrapper = wrap.map(|one| one.local().to_string());
            let wrapped = wrap.map(attr_map).unwrap_or(Value::Null);
            match wrapper.as_deref() {
                Some("hyperlink") => wrap_link += 1,
                Some("ins") => wrap_ins += 1,
                Some("del") => wrap_del += 1,
                _ => {}
            }
            if wrapper.is_some() {
                with_wrap += 1;
            }
            entries.push(json!({
                "para": para,
                "at": at,
                "text": text,
                "props_written": props.is_some(),
                "props_attrs": props.map(attr_map).unwrap_or(Value::Null),
                "elements": kids
                    .iter()
                    .map(|one| one.local().to_string())
                    .collect::<Vec<String>>(),
                "switches": Value::Object(switches),
                "values": Value::Object(values),
                "format": kids
                    .iter()
                    .filter(|one| one.local() != "rStyle")
                    .map(|one| json!({"element": one.local(), "written": attr_map(one)}))
                    .collect::<Vec<Value>>(),
                "style": named,
                "style_found": named.as_ref().map(|_| hit.is_some()),
                "style_name": hit.and_then(|one| one.1.clone()),
                "style_parent": hit.and_then(|one| one.2.clone()),
                "style_format": hit.map(|one| Value::Array(one.3.clone())).unwrap_or(Value::Null),
                "style_switches": Value::Object(char_switches),
                "contents": body
                    .iter()
                    .map(|one| json!({"element": one.local(), "written": attr_map(one)}))
                    .collect::<Vec<Value>>(),
                "refs": Value::Object(refs),
                "note": note,
                "wrapped": wrapper,
                "wrapped_written": wrapped,
                "breaks": Value::Array(breaks),
                "instructions": Value::Array(instructions),
                "field_chars": Value::Array(fields),
            }));
        }
    }
    let referenced: Vec<(String, String)> = {
        let mut unique: Vec<(String, String)> = Vec::new();
        for one in seen_refs.iter() {
            if !unique.contains(one) {
                unique.push(one.clone());
            }
        }
        unique
    };
    json!({
        "checked": checked,
        "listed": entries.len(),
        "with_props": with_props,
        "props_empty": props_empty,
        "with_format": with_format,
        "with_style": with_style,
        "style_found": style_found,
        "where_both_spoke": where_both,
        "runs_with_text": with_text,
        "runs_with_ref": with_ref,
        "ref_found": ref_found,
        "field_runs": field_runs,
        "runs_wrapped": with_wrap,
        "wrapped_hyperlink": wrap_link,
        "wrapped_ins": wrap_ins,
        "wrapped_del": wrap_del,
        "notes_in_parts": notes.len(),
        "notes_referenced": referenced.len(),
        "notes_unreferenced": notes.len() - referenced.len(),
        "bold_on": on[0], "bold_off": off[0], "bold_from_style": from_style[0],
        "italic_on": on[1], "italic_off": off[1], "italic_from_style": from_style[1],
        "strike_on": on[2], "strike_off": off[2], "strike_from_style": from_style[2],
        "underline_on": on[3], "underline_off": off[3], "underline_from_style": from_style[3],
        "list": entries.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// 一条 span 自己**直接**带的那些字（套在里面的那一串算里面那条的）
fn own_text(holder: &xmlscan::Node) -> String {
    holder
        .children
        .iter()
        .filter(|one| one.local() == "#text")
        .map(|one| one.text())
        .collect::<Vec<String>>()
        .join("")
}

/// 段里的「串」按文件的顺序摊平。`text:span` 可以套 `text:span`（实测 LibreOffice 把
/// 「样式说斜、段上自己说粗」写成外面一层点 `Emphasis`、里面一层点 `T1`），
/// 所以这一趟是递归的，`depth` 说这条在第几层；不是 span 的孩子（注、软分页、书签…）
/// 整块跳过 —— 注有自己那份账，不在带它的那一段里再算一遍。
/// 套在 span 里的那些字**只算在外层那一条的 text 上**，不再单独出一条不包起来的字：
/// 同一句话在两层各出现一次，条数就成了读者造出来的
struct Piece<'a> {
    depth: usize,
    span: Option<&'a xmlscan::Node>,
    text: String,
}

fn collect_pieces<'a>(
    holder: &'a xmlscan::Node,
    depth: usize,
    bare_text: bool,
    out: &mut Vec<Piece<'a>>,
) {
    for one in &holder.children {
        if one.local() == "span" {
            out.push(Piece {
                depth,
                span: Some(one),
                text: own_text(one),
            });
            collect_pieces(one, depth + 1, false, out);
        } else if bare_text && one.local() == "#text" {
            out.push(Piece {
                depth,
                span: None,
                text: one.text(),
            });
        }
    }
}

/// ODF 的字符格式：段上的字分成 `text:span`（各点一个样式名）与**夹在中间不包起来的字**。
///
/// 那一串不包起来的字是这个家族的第三种情况：它不是「有一个串而里面没格式」，
/// 而是文件压根没给那一段立一个元素，所以它的 `style` 与 `resolved` 都交 null
/// （没有号可查，不是查不到），`element` 写 `#text`。
/// 样式名那一跳找**两段**：先在 content.xml（LibreOffice 把 `T1`…`T12` 这些自动样式
/// 写在这一个部件里），找不到再去 styles.xml（命名的字符样式住那儿），
/// 而且把住在哪个部件交出来（`found_in`）—— 两处同名时取前者，那是文件里正文可见的那一份。
/// 值全在样式的 `style:text-properties` 上，按文件写的名字原样交（`fo:` 与 `style:` 留着）；
/// 父样式链**不跟**（`parent` 只报出来），而 `display` 是文件自己写的显示名 ——
/// 实测 `Strong_20_Emphasis` 那一个号对应的名字是「Strong Emphasis」，号与名不是一回事。
/// span 会套 span（外面那层点样式、里面那层点直接格式），所以每条带 `depth`，
/// 而一条 span 自己的 `text` 只算它直接带的那些字，不然同一句话在两层各出现一次。
fn odt_run_formats(
    paragraphs: &[&xmlscan::Node],
    root: &xmlscan::Node,
    styles: Option<&xmlscan::Node>,
    limit: usize,
) -> Value {
    let mut found: Vec<(
        String,
        Option<String>,
        Option<Value>,
        Option<String>,
        String,
    )> = Vec::new();
    for (part, holder) in [("content", Some(root)), ("styles", styles)].into_iter() {
        let Some(tree) = holder else { continue };
        for one in tree.descendants("style").into_iter() {
            if one.attr_local("family") != Some("text") {
                continue;
            }
            let Some(name) = one.attr_local("name") else {
                continue;
            };
            found.push((
                name.to_string(),
                one.attr_local("parent-style-name").map(String::from),
                one.child("text-properties").map(kept_attrs),
                one.attr_local("display-name").map(String::from),
                part.to_string(),
            ));
        }
    }
    let mut entries: Vec<Value> = Vec::new();
    let mut spans = 0usize;
    let mut nested = 0usize;
    let mut bare = 0usize;
    let mut resolved = 0usize;
    let mut with_format = 0usize;
    let mut on = [0usize; 4];
    let mut off = [0usize; 4];
    for (para, holder) in paragraphs.iter().enumerate() {
        let mut pieces: Vec<Piece> = Vec::new();
        collect_pieces(holder, 1, true, &mut pieces);
        for (at, piece) in pieces.iter().enumerate() {
            let Some(node) = piece.span else {
                bare += 1;
                // 不包起来的那些字没有「串上自己写格式」这回事：四个问题都没有谁说过话，
                // 所以四格各交 null —— 与 span 那一行同一个形状。「缺键」留给
                // 「这一族我没看」，不能拿来表示「看了，没人说」
                let mut switches = serde_json::Map::new();
                for pair in ODF_RUN_SWITCHES.iter() {
                    switches.insert(pair.1.to_string(), Value::Null);
                }
                entries.push(json!({
                    "para": para,
                    "at": at,
                    "element": "#text",
                    "depth": piece.depth,
                    "text": piece.text,
                    "style": Value::Null,
                    "resolved": Value::Null,
                    "found_in": Value::Null,
                    "parent": Value::Null,
                    "display": Value::Null,
                    "written": Value::Null,
                    "switches": Value::Object(switches),
                }));
                continue;
            };
            spans += 1;
            if piece.depth > 1 {
                nested += 1;
            }
            let want = node.attr_local("style-name");
            let hit = want.and_then(|raw| found.iter().find(|one| one.0 == raw));
            if hit.is_some() {
                resolved += 1;
            }
            let written = hit.and_then(|one| one.2.clone());
            if written
                .as_ref()
                .and_then(|had| had.as_object())
                .is_some_and(|had| !had.is_empty())
            {
                with_format += 1;
            }
            let mut switches = serde_json::Map::new();
            for (which, pair) in ODF_RUN_SWITCHES.iter().enumerate() {
                let read = odf_run_switch(written.as_ref(), pair.0);
                if read.as_bool() == Some(true) {
                    on[which] += 1;
                } else if read.as_bool() == Some(false) {
                    off[which] += 1;
                }
                switches.insert(pair.1.to_string(), read);
            }
            entries.push(json!({
                "para": para,
                "at": at,
                "element": "span",
                "depth": piece.depth,
                "text": piece.text,
                "style": want.map(String::from),
                "resolved": hit.is_some(),
                "found_in": hit.map(|one| one.4.clone()),
                "parent": hit.and_then(|one| one.1.clone()),
                "display": hit.and_then(|one| one.3.clone()),
                "written": written,
                "switches": Value::Object(switches),
            }));
        }
    }
    json!({
        "checked": spans + bare,
        "listed": entries.len(),
        "spans": spans,
        "nested_spans": nested,
        "bare_text": bare,
        "resolved": resolved,
        "with_format": with_format,
        "bold_on": on[0], "bold_off": off[0],
        "italic_on": on[1], "italic_off": off[1],
        "strike_on": on[2], "strike_off": off[2],
        "underline_on": on[3], "underline_off": off[3],
        "list": entries.into_iter().take(limit).collect::<Vec<Value>>(),
    })
}

/// ODF 的分栏：`text:section` 点名一个 family=section 的样式，栏在它的
/// `style:section-properties` 里，而且每一栏还各写一份 `style:column`（相对宽度）
fn odf_columns(root: &xmlscan::Node, limit: usize) -> Value {
    let mut styles: Vec<(String, Option<Value>, Vec<Value>, Option<String>, bool)> = Vec::new();
    for one in root.descendants("style").into_iter() {
        if one.attr_local("family") != Some("section") {
            continue;
        }
        let Some(name) = one.attr_local("name") else {
            continue;
        };
        let props = one.child("section-properties");
        let cols = props.and_then(|kid| kid.child("columns"));
        let parts: Vec<Value> = cols
            .map(|kid| {
                kid.children
                    .iter()
                    .filter(|had| had.local() == "column")
                    .map(kept_attrs)
                    .collect::<Vec<Value>>()
            })
            .unwrap_or_default();
        styles.push((
            name.to_string(),
            cols.map(kept_attrs),
            parts,
            props
                .and_then(|kid| kid.attr_local("dont-balance-text-columns"))
                .map(String::from),
            props.is_some(),
        ));
    }
    let mut entries: Vec<Value> = Vec::new();
    for one in root.descendants("section").into_iter().take(limit) {
        let name = one.attr_local("style-name");
        let found = name.and_then(|want| styles.iter().find(|one| one.0 == want));
        entries.push(json!({
            "name": one.attr_local("name").map(String::from),
            "style": name.map(String::from),
            "resolved": found.is_some(),
            "has_properties": found.map(|one| one.4).unwrap_or(false),
            "written": found.and_then(|one| one.1.clone()).unwrap_or(Value::Null),
            "parts": found.map(|one| Value::Array(one.2.clone())).unwrap_or(Value::Array(Vec::new())),
            "dont_balance": found.and_then(|one| one.3.clone()),
        }));
    }
    let written = entries
        .iter()
        .filter(|one| !one["written"].is_null())
        .count();
    json!({"sections": entries.len(), "written": written, "list": entries})
}

/// `w:numPr` 里那两个开关（`w:ilvl` 与 `w:numId`）：文件写了哪个就交哪个，都不写交两个 null
///
/// 那个 `w:numPr` 元素在、里面两个开关都不写，也算「这一段说过话」—— 那是另一件事，
/// 由调用方看 `holder.child("numPr")` 自己判，这里不替它补默认值
fn docx_num_pr(holder: &xmlscan::Node) -> (Option<String>, Option<String>) {
    let inner = holder.child("numPr");
    let ilvl = inner
        .and_then(|kid| kid.child("ilvl"))
        .and_then(|one| one.attr_local("val"))
        .map(String::from);
    let num_id = inner
        .and_then(|kid| kid.child("numId"))
        .and_then(|one| one.attr_local("val"))
        .map(String::from);
    (ilvl, num_id)
}

/// 一个元素下那些「子元素各带一个 `w:val`」的写法收成一张表（`w:numFmt`、`w:nsid` 都是这种）
///
/// 同名只取第一条：两份 `w:numFmt` 出现在同一份定义里是文件自己矛盾，不替它挑后面的
fn docx_val_children(node: &xmlscan::Node) -> Value {
    let mut out = serde_json::Map::new();
    for kid in node.children.iter() {
        if matches!(kid.local(), "pPr" | "rPr") {
            continue;
        }
        if out.contains_key(kid.local()) {
            continue;
        }
        if let Some(val) = kid.attr_local("val") {
            out.insert(kid.local().to_string(), json!(val));
        }
    }
    Value::Object(out)
}

/// 一份 `w:lvl` 写了什么：格式串与起始值在 `written` 里，缩进与字体另交两份属性表 ——
/// 圆点那一级 `w:rPr/w:rFonts w:ascii="Symbol"` 不是装饰，它说的是 `w:lvlText` 里那个
/// `U+F0B7` 要按 Symbol 那张字模才画得出来（当普通字符交回去就是一枚看不见的方块）
fn docx_level_written(node: &xmlscan::Node) -> Value {
    json!({
        "ilvl": node.attr_local("ilvl").map(String::from),
        "written": docx_val_children(node),
        "indent": node
            .child("pPr")
            .and_then(|kid| kid.child("ind"))
            .map(attr_map)
            .unwrap_or(Value::Null),
        "fonts": node
            .child("rPr")
            .and_then(|kid| kid.child("rFonts"))
            .map(attr_map)
            .unwrap_or(Value::Null),
    })
}

/// docx 的编号：那一路要跳三跳，而且**不从段上开始也走得通**
///
/// 段上的 `w:numPr` 只是三个来源之一：Word 与 python-docx 都会把编号放在**段点名的样式**里
/// （`w:style/w:pPr/w:numPr`），而 `w:abstractNum` 里那一条 `w:lvl` 又反过来写着
/// `<w:pStyle w:val="ListNumber"/>` —— 样式与编号是一个环，这里挑「段 → 样式 → num →
/// abstract → lvl」这一条边走，挑了哪一条记在哪一段的 `from` 上。
/// `w:num` 与 `w:abstractNum` 的号是**分开编的**（实测 `numId 1 → abstractNumId 8`），
/// 所以自报数这一族没有：`w:num` 上不写 count，条数只能数。
fn docx_numbering(
    paragraphs: &[&xmlscan::Node],
    numbering: Option<&xmlscan::Node>,
    style_root: Option<&xmlscan::Node>,
    has_part: bool,
    limit: usize,
) -> Value {
    // `parse_str` 交回来的是伪根 `#doc`，它只有一个孩子（`w:numbering` / `w:styles`）——
    // 在这一层找 `w:num` 永远找不到，条数会平白无故报 0（CI 上就是这条抓到的）
    let numbering = numbering.map(|root| root.child("numbering").unwrap_or(root));
    let style_root = style_root.map(|root| root.child("styles").unwrap_or(root));
    // numId -> 那一条 `w:num` 写的 abstractNumId
    let mut nums: Vec<(String, Option<String>)> = Vec::new();
    // abstractNumId -> 那一份定义写了哪几级（每级一份账）
    let mut abstracts: Vec<(String, Option<Value>, Vec<Value>)> = Vec::new();
    if let Some(root) = numbering {
        for one in root.all("num").into_iter() {
            let Some(id) = one.attr_local("numId") else {
                continue;
            };
            if nums.iter().any(|had| had.0 == id) {
                continue;
            }
            nums.push((
                id.to_string(),
                one.child("abstractNumId")
                    .and_then(|kid| kid.attr_local("val"))
                    .map(String::from),
            ));
        }
        for one in root.all("abstractNum").into_iter() {
            let Some(id) = one.attr_local("abstractNumId") else {
                continue;
            };
            if abstracts.iter().any(|had| had.0 == id) {
                continue;
            }
            let levels: Vec<Value> = one
                .all("lvl")
                .into_iter()
                .take(limit)
                .map(docx_level_written)
                .collect();
            abstracts.push((id.to_string(), Some(docx_val_children(one)), levels));
        }
    }
    // 样式那一路：`w:styleId` -> 那份样式替它段的 ilvl / numId
    let mut style_nums: Vec<(String, Option<String>, Option<String>)> = Vec::new();
    if let Some(root) = style_root {
        for one in root.all("style").into_iter() {
            let Some(id) = one.attr_local("styleId") else {
                continue;
            };
            if style_nums.iter().any(|had| had.0 == id) {
                continue;
            }
            let (ilvl, num_id) = match one.child("pPr") {
                Some(holder) => docx_num_pr(holder),
                None => (None, None),
            };
            style_nums.push((id.to_string(), ilvl, num_id));
        }
    }
    let mut entries: Vec<Value> = Vec::new();
    let mut used: Vec<String> = Vec::new();
    let mut on_paragraph = 0usize;
    let mut via_style = 0usize;
    let mut both = 0usize;
    let mut unresolved = 0usize;
    for (index, one) in paragraphs.iter().enumerate() {
        let Some(holder) = one.child("pPr") else {
            continue;
        };
        let (ilvl, num_id) = docx_num_pr(&holder);
        let para_has = holder.child("numPr").is_some();
        let style = holder.child("pStyle").and_then(|kid| kid.attr_local("val"));
        let found = style.and_then(|want| style_nums.iter().find(|had| had.0 == want));
        let style_has = found.map(|one| one.2.is_some()).unwrap_or(false);
        if !para_has && !style_has {
            continue;
        }
        let from = match (para_has, style_has) {
            (true, true) => {
                both += 1;
                "both"
            }
            (true, false) => {
                on_paragraph += 1;
                "paragraph"
            }
            (false, true) => {
                via_style += 1;
                "style"
            }
            (false, false) => "",
        };
        let chosen_id = num_id
            .clone()
            .or_else(|| found.and_then(|one| one.2.clone()));
        let chosen_ilvl = ilvl.clone().or_else(|| found.and_then(|one| one.1.clone()));
        let held = chosen_id
            .as_ref()
            .and_then(|want| nums.iter().find(|had| &had.0 == want));
        let abstract_id = held.and_then(|one| one.1.clone());
        let definition = abstract_id
            .as_ref()
            .and_then(|want| abstracts.iter().find(|had| &had.0 == want));
        let level = definition.and_then(|one| {
            chosen_ilvl
                .as_ref()
                .and_then(|lvl| one.2.iter().find(|had| had["ilvl"] == json!(lvl)).cloned())
        });
        if let Some(one) = chosen_id.as_ref() {
            if !used.iter().any(|had| had == one) {
                used.push(one.clone());
            }
        }
        if chosen_id.is_some() && held.is_none() {
            unresolved += 1;
        }
        entries.push(json!({
            "index": index,
            "style": style.map(String::from),
            "from": from,
            "num_id": chosen_id,
            "ilvl": chosen_ilvl,
            "para_num_id": num_id,
            "style_num_id": found.and_then(|one| one.2.clone()),
            "abstract": abstract_id,
            "resolved": held.is_some(),
            "abstract_found": definition.is_some(),
            "level_found": level.is_some(),
            "level": level.unwrap_or(Value::Null),
        }));
    }
    let definitions: Vec<Value> = nums
        .iter()
        .take(limit)
        .map(|one| {
            let held = one
                .1
                .as_ref()
                .and_then(|want| abstracts.iter().find(|had| &had.0 == want));
            json!({
                "num_id": one.0,
                "abstract": one.1,
                "abstract_found": held.is_some(),
                "written": held.and_then(|one| one.1.clone()).unwrap_or(Value::Null),
                "referenced": used.iter().any(|had| *had == one.0),
                "levels": held.map(|one| one.2.clone()).unwrap_or_default(),
            })
        })
        .collect();
    json!({
        "part": has_part,
        "nums": nums.len(),
        "abstracts": abstracts.len(),
        "checked": paragraphs.len(),
        "listed": entries.len(),
        "on_paragraph": on_paragraph,
        "via_style": via_style,
        "both": both,
        "unresolved": unresolved,
        "used": used,
        "list": entries.into_iter().take(limit).collect::<Vec<Value>>(),
        "definitions": definitions,
    })
}

/// ODF：走一遍正文，按 `text:list` 的嵌套层数给每一段记一个深度
///
/// 段的口径与 `office_text::odf_paragraphs` 一条不差（只收 `text:p`，绕开批注与修订表），
/// 不然这里的 `index` 与那份段账对不上。`chain` 是这一层套着的每个 `text:list`
/// 自己写的 `text:style-name`（**没有就是 null，不是没看** —— 套在里面那一层常常不写）
fn odf_list_depths(
    node: &xmlscan::Node,
    depth: usize,
    chain: &mut Vec<Option<String>>,
    into: &mut Vec<(usize, Vec<Option<String>>)>,
) {
    for one in &node.children {
        let local = one.local();
        if local == "annotation" || local == "tracked-changes" {
            continue;
        }
        if local == "p" {
            into.push((depth, chain.clone()));
            continue;
        }
        if local == "list" {
            chain.push(crate::odsheet::attr_of(one, "style-name").map(String::from));
            odf_list_depths(one, depth + 1, chain, into);
            chain.pop();
            continue;
        }
        odf_list_depths(one, depth, chain, into);
    }
}

/// ODF 的编号：级别不是属性，是**嵌套**；而那一份定义经常住在另一个部件里
///
/// `text:list` 套几层就是第几级（docx 那边写的是 `w:ilvl`，而且是 **0 基**，这一族
/// `text:level` 是 **1 基** —— 两个数不是一回事，所以这里只交嵌套层数与文件自己写的
/// `text:level`，不折算）。列表样式的名有两个地方写：段点名的样式上
/// （`style:style/@text:list-style-name`）与 `text:list` 元素自己（`@text:style-name`），
/// 两个都交。实测 LibreOffice 的 ODF 导出把 `text:list-style` 的定义**全写在 styles.xml**，
/// 段样式在 content.xml —— 这一跳是跨部件的，所以每条都带一个 `part` 说清在哪份件里找到的。
fn odf_numbering(
    paragraphs: &[&xmlscan::Node],
    text_body: &xmlscan::Node,
    content_root: &xmlscan::Node,
    style_root: Option<&xmlscan::Node>,
    limit: usize,
) -> Value {
    let mut depths: Vec<(usize, Vec<Option<String>>)> = Vec::new();
    let mut chain: Vec<Option<String>> = Vec::new();
    odf_list_depths(text_body, 0, &mut chain, &mut depths);
    let mut roots: Vec<(&xmlscan::Node, &'static str)> = vec![(content_root, "content.xml")];
    if let Some(one) = style_root {
        roots.push((one, "styles.xml"));
    }
    // 段样式名 -> 它点名的列表样式名 + 在哪个部件找到的
    let mut para_styles: Vec<(String, Option<String>, &'static str)> = Vec::new();
    // 列表样式名 -> 它写了几级（每级一份账）+ 在哪个部件
    let mut list_styles: Vec<(String, usize, Vec<Value>, &'static str)> = Vec::new();
    for (root, part) in roots.into_iter() {
        for one in root.descendants("style").into_iter() {
            let Some(name) = crate::odsheet::attr_of(one, "name") else {
                continue;
            };
            if para_styles.iter().any(|had| had.0 == name) {
                continue;
            }
            para_styles.push((
                name.to_string(),
                crate::odsheet::attr_of(one, "list-style-name").map(String::from),
                part,
            ));
        }
        for one in root.descendants("list-style").into_iter() {
            let Some(name) = crate::odsheet::attr_of(one, "name") else {
                continue;
            };
            if list_styles.iter().any(|had| had.0 == name) {
                continue;
            }
            let kids = one.children.len();
            let levels: Vec<Value> = one
                .children
                .iter()
                .take(limit)
                .map(|kid| {
                    json!({
                        "kind": kid.local(),
                        "level": crate::odsheet::attr_of(kid, "level").map(String::from),
                        "written": kept_attrs(kid),
                    })
                })
                .collect();
            list_styles.push((name.to_string(), kids, levels, part));
        }
    }
    let mut entries: Vec<Value> = Vec::new();
    let mut in_list = 0usize;
    let mut max_depth = 0usize;
    let mut resolved = 0usize;
    for (index, one) in paragraphs.iter().enumerate() {
        let (depth, names) = depths
            .get(index)
            .cloned()
            .unwrap_or_else(|| (0, Vec::new()));
        let style = crate::odsheet::attr_of(one, "style-name").map(String::from);
        let found = style
            .as_ref()
            .and_then(|want| para_styles.iter().find(|had| &had.0 == want));
        let named = found.and_then(|one| one.1.clone());
        let style_part = found.map(|one| one.2);
        if depth == 0 && named.is_none() {
            continue;
        }
        let held = named
            .as_ref()
            .and_then(|want| list_styles.iter().find(|had| &had.0 == want));
        if depth > 0 {
            in_list += 1;
        }
        if depth > max_depth {
            max_depth = depth;
        }
        if held.is_some() {
            resolved += 1;
        }
        // 嵌套层数与文件自己写的 `text:level` 对上才对，对不上交 null（1 基不是 0 基）
        let level = held.and_then(|one| {
            one.2
                .iter()
                .find(|had| {
                    had["level"]
                        .as_str()
                        .and_then(|raw| raw.parse::<usize>().ok())
                        == Some(depth)
                })
                .cloned()
        });
        entries.push(json!({
            "index": index,
            "style": style,
            "style_part": style_part,
            "list_style": named,
            "list_part": held.map(|one| one.3),
            "depth": depth,
            "chain": names,
            "resolved": held.is_some(),
            "level_found": level.is_some(),
            "level": level.unwrap_or(Value::Null),
        }));
    }
    let definitions: Vec<Value> = list_styles
        .iter()
        .take(limit)
        .map(|one| {
            json!({
                "name": one.0,
                "found": one.1,
                "part": one.3,
                "levels": one.2,
            })
        })
        .collect();
    json!({
        "lists": text_body.descendants("list").len(),
        "items": text_body.descendants("list-item").len(),
        "checked": paragraphs.len(),
        "listed": entries.len(),
        "in_list": in_list,
        "max_depth": max_depth,
        "resolved": resolved,
        "styles": list_styles.len(),
        "in_content": list_styles.iter().filter(|one| one.3 == "content.xml").count(),
        "in_styles": list_styles.iter().filter(|one| one.3 == "styles.xml").count(),
        "list": entries.into_iter().take(limit).collect::<Vec<Value>>(),
        "definitions": definitions,
    })
}

/// docx 这张表多宽：三本账各交各的，谁也不替谁圆场
///
/// `w:tblPr/w:tblW` 说的是「这张表自己要多宽」，而 python-docx 与 Word 常写
/// `type="auto" w="0"` —— 那是一个**说了等于没说**的值（LibreOffice 重写同一份件时
/// 会把它换成实数 `w="8640" type="dxa"`，两份都照文件交）；`w:tblGrid/w:gridCol@w`
/// 是网格给几列各多宽；每一格自己的 `w:tcPr/w:tcW` 又是第三个数，而且横向合并那格
/// 自己写的是**两列之和**（实测 5760 对网格的 2880+2880）。行与格只数这张表自己的
/// 直接孩子（`all`），套在格里的另一张表不算在这一张的账上。
///
/// 负的宽度这一族没量过：镜像那份只认非负的串，真来了负的会红在 CI 上，
/// 不会静悄悄交出两个不同的数
fn docx_table_layouts(body: &xmlscan::Node, limit: usize) -> Value {
    let mut entries: Vec<Value> = Vec::new();
    let mut said = 0usize;
    let mut auto_said = 0usize;
    let mut sum_grid = 0i64;
    let mut shade_cells = 0usize;
    let mut border_cells = 0usize;
    let mut empty_border_cells = 0usize;
    let mut align_cells = 0usize;
    for (at, tbl) in body.descendants("tbl").into_iter().take(limit).enumerate() {
        let props = tbl.child("tblPr");
        let width = props.and_then(|one| one.child("tblW"));
        let raw = width.and_then(|one| one.attr_local("w")).map(String::from);
        let kind = width
            .and_then(|one| one.attr_local("type"))
            .map(String::from);
        if width.is_some() {
            said += 1;
        }
        if kind.as_deref() == Some("auto") {
            auto_said += 1;
        }
        let grid: Vec<Value> = match tbl.child("tblGrid") {
            Some(holder) => holder
                .children
                .iter()
                .filter(|one| one.local() == "gridCol")
                .map(attr_map)
                .collect(),
            None => Vec::new(),
        };
        let mut cells: Vec<Value> = Vec::new();
        let rows = tbl.all("tr");
        for (row, tr) in rows.iter().enumerate() {
            for (col, tc) in tr.all("tc").into_iter().enumerate() {
                let tc_pr = tc.child("tcPr");
                let one_width = tc_pr.and_then(|one| one.child("tcW")).map(attr_map);
                let span = tc_pr
                    .and_then(|one| one.child("gridSpan"))
                    .and_then(|one| one.attr_local("val"))
                    .map(String::from);
                let shading = tc_pr.and_then(|one| one.child("shd")).map(attr_map);
                let edges = tc_pr.and_then(|one| one.child("tcBorders"));
                let border_map: Value = match edges {
                    Some(holder) => Value::Object(
                        holder
                            .children
                            .iter()
                            .filter(|one| {
                                one.attr_local("val").is_some() || one.attr_local("color").is_some()
                            })
                            .map(|one| (one.local().to_string(), attr_map(one)))
                            .collect::<serde_json::Map<String, Value>>(),
                    ),
                    None => Value::Null,
                };
                let align = tc_pr
                    .and_then(|one| one.child("vAlign"))
                    .and_then(|one| one.attr_local("val"))
                    .map(String::from);
                // 上榜的条件：这一格自己说过话（宽、跨列、底色、对齐，或至少写了一条边）
                let has_edge = edges
                    .map(|holder| !holder.children.is_empty())
                    .unwrap_or(false);
                if one_width.is_none()
                    && span.is_none()
                    && shading.is_none()
                    && align.is_none()
                    && !has_edge
                {
                    continue;
                }
                shade_cells += usize::from(shading.is_some());
                border_cells += usize::from(
                    border_map
                        .as_object()
                        .map(|one| !one.is_empty())
                        .unwrap_or(false),
                );
                empty_border_cells += usize::from(
                    edges
                        .map(|holder| holder.children.is_empty())
                        .unwrap_or(false),
                );
                align_cells += usize::from(align.is_some());
                cells.push(json!({
                    "row": row,
                    "col": col,
                    "written": one_width,
                    "span": span,
                    "shading": shading,
                    "borders": border_map,
                    "borders_present": edges.is_some(),
                    "valign": align,
                }));
            }
        }
        // 网格那一本加起来是多少（单位 twips 换成 0.01mm 再累加，坏值跳过不猜）
        for one in &grid {
            if let Some(raw) = one["w"].as_str() {
                sum_grid += crate::paper::twips(raw).unwrap_or(0);
            }
        }
        entries.push(json!({
            "at": at,
            "written": width.map(attr_map).unwrap_or(Value::Null),
            "w": raw,
            "kind": kind,
            "mm": width
                .and_then(|one| one.attr_local("w"))
                .and_then(crate::paper::twips),
            "align": props
                .and_then(|one| one.child("jc"))
                .and_then(|one| one.attr_local("val"))
                .map(String::from),
            "indent": props
                .and_then(|one| one.child("tblInd"))
                .map(attr_map)
                .unwrap_or(Value::Null),
            "layout": props
                .and_then(|one| one.child("tblLayout"))
                .and_then(|one| one.attr_local("type"))
                .map(String::from),
            "cell_mar": props.and_then(|one| one.child("tblCellMar")).is_some(),
            "rows": rows.len(),
            "grid": grid,
            "cols": tbl
                .child("tblGrid")
                .map(|holder| {
                    holder
                        .children
                        .iter()
                        .filter(|one| one.local() == "gridCol")
                        .count()
                })
                .unwrap_or(0),
            "cells": cells.into_iter().take(limit).collect::<Vec<Value>>(),
        }));
    }
    json!({
        "listed": entries.len(),
        "with_tblW": said,
        "auto": auto_said,
        "grid_sum": sum_grid,
        "shade_cells": shade_cells,
        "border_cells": border_cells,
        "empty_border_cells": empty_border_cells,
        "align_cells": align_cells,
        "list": entries,
    })
}

/// ODF 那一族的「边」写在哪些属性名上（`fo:border-top` 这种）
///
/// 只收这几个名字：同一份 `style:table-cell-properties` 里还有
/// `style:border-line-width-top`（**每根线**多宽 —— 双线 LO 写的 2.25pt 是三根合起来的）
/// 与 `fo:padding-*`（连默认值都写出来），把它们混进「有几条边」就把账读错了
const CELL_EDGES: [&str; 9] = [
    "fo:border",
    "fo:border-left",
    "fo:border-right",
    "fo:border-top",
    "fo:border-bottom",
    "fo:border-before",
    "fo:border-after",
    "fo:border-start",
    "fo:border-end",
];

/// 一条边的值里到底有没有线：`none` 与 `hidden` 是关键字，
/// 写成三段式（`0.0pt none #000000`）时那个关键字也还在，所以按空格拆开看
fn edge_lined(raw: &str) -> bool {
    !raw.split_whitespace()
        .any(|one| one.eq_ignore_ascii_case("none") || one.eq_ignore_ascii_case("hidden"))
}

/// RTF 那份字符格式账（`crate::rtf` 那一趟走出来的逐串账本）。
///
/// 这一族**没有**「一串字」那个元素：格式写在群头上，所以「有串而没说格式」在这里
/// 判不住 —— `with_props` 与 `props_empty` 交 null，而 `checked` 只数那些自己说过
/// 格式控制字的群。段前缀那一层（所有群之外）说过的控制字另记在 `words_outside_groups`：
/// 那一条归属于段，LibreOffice 每一段都重发一份样式默认值，那个数就是这份件里
/// 「段落级重发」的条数。号（`\cfN` / `\fN`）在文件自己那两张表上跳一跳，
/// 跳不通的那一格交 `resolved: false` 与原样那个号
fn rtf_run_formats(had: &crate::rtf::Rtf, limit: usize) -> Value {
    let rows = &had.run_rows;
    let mut on = [0usize; 4];
    let mut off = [0usize; 4];
    for one in rows.iter() {
        for (which, key) in ["bold", "italic", "strike", "underline"].iter().enumerate() {
            match one["switches"][*key].as_bool() {
                Some(true) => on[which] += 1,
                Some(false) => off[which] += 1,
                _ => {}
            }
        }
    }
    json!({
        "checked": rows.len(),
        "listed": rows.len().min(limit),
        "with_props": Value::Null,
        "props_empty": Value::Null,
        "with_format": rows.len(),
        "words_outside_groups": had.run_words_stray,
        "bold_on": on[0], "bold_off": off[0],
        "italic_on": on[1], "italic_off": off[1],
        "strike_on": on[2], "strike_off": off[2],
        "underline_on": on[3], "underline_off": off[3],
        "list": rows.iter().take(limit).cloned().collect::<Vec<Value>>(),
    })
}

/// RTF 那份编号账的封顶：`list` 与 `definitions` 各取前 `limit` 条，
/// 而所有计数在截断之前就算好了（与别的账同一个口径 —— 数是一起数的，列是列几条）
fn rtf_numbering(had: &Value, limit: usize) -> Value {
    let mut out = match had.as_object() {
        Some(one) => one.clone(),
        None => return had.clone(),
    };
    for key in ["list", "definitions", "override_list"] {
        let cut = out
            .get(key)
            .and_then(|one| one.as_array())
            .map(|items| Value::Array(items.iter().take(limit).cloned().collect::<Vec<Value>>()));
        if let Some(one) = cut {
            out.insert(key.to_string(), one);
        }
    }
    Value::Object(out)
}

/// ODF 这张表多宽：一条 `table:table-column` 可以顶好几列，宽度还在一跳之外的样式上
///
/// 与 docx 三处不同：① 列是 `table:table-column` 元素，`number-columns-repeated` 说它
/// 顶几列（实测 LibreOffice 把两列写成**一条** `repeated="2"`，所以「几条」与「几列宽」
/// 是两本账）；② 宽度不写在表上，一跳在 `style:style`（family=table-column）的
/// `style:table-column-properties/@style:column-width` 上，而那份样式**在 content.xml**
/// （与编号那一批把定义搬去 styles.xml 正好相反，所以每一条都带 `part`）；
/// ③ 表自己的宽度在 family=table 的样式的 `style:table-properties/@style:width` 上，
/// 单位是自带单位的串（`15.24cm`），与 docx 的 twips 换成同一个 0.01mm 才可比。
/// ④ 格子的底色 / 边 / 垂直对齐同样一跳在样式上：`table:table-cell/@table:style-name`
/// 点名一份 family=table-cell 的**自动样式**（LibreOffice 按地址起名：`表格1.A1`），
/// 值在那份样式的 `style:table-cell-properties` 里 —— 这一族没有「写在格子上」的地方。
/// 底色是 `fo:background-color`（值小写并带 `#`，docx 那边是 `w:fill="FFFF00"`），
/// 边是 `fo:border-<方位>`（一整串 `<宽度> <样式> <颜色>`），垂直对齐是
/// `style:vertical-align`。被合并掉的格子写成 `table:covered-table-cell`：它没有内容，
/// 「几个格子元素」与「其中几个是占位的」是两本账（docx 那边对应 `w:gridSpan` / `w:vMerge`）。
fn odf_table_layouts(
    text_body: &xmlscan::Node,
    content_root: &xmlscan::Node,
    style_root: Option<&xmlscan::Node>,
    limit: usize,
) -> Value {
    let mut roots: Vec<(&xmlscan::Node, &'static str)> = vec![(content_root, "content.xml")];
    if let Some(one) = style_root {
        roots.push((one, "styles.xml"));
    }
    // family=table-column 的样式名 -> 那份属性表 + 在哪份件里
    let mut columns: Vec<(String, Option<Value>, &'static str)> = Vec::new();
    let mut tables_style: Vec<(String, Option<Value>, &'static str)> = Vec::new();
    // family=table-cell 的样式名 -> 那份属性表（底色、边、垂直对齐都在这）+ 在哪份件里
    let mut cell_styles: Vec<(String, Option<Value>, &'static str)> = Vec::new();
    for (root, part) in roots.into_iter() {
        for one in root.descendants("style").into_iter() {
            let Some(name) = crate::odsheet::attr_of(one, "name") else {
                continue;
            };
            match crate::odsheet::attr_of(one, "family") {
                Some("table-column") => {
                    if !columns.iter().any(|had| had.0 == name) {
                        let props = one.child("table-column-properties").map(kept_attrs);
                        columns.push((name.to_string(), props, part));
                    }
                }
                Some("table") => {
                    if !tables_style.iter().any(|had| had.0 == name) {
                        let props = one.child("table-properties").map(kept_attrs);
                        tables_style.push((name.to_string(), props, part));
                    }
                }
                Some("table-cell") => {
                    if !cell_styles.iter().any(|had| had.0 == name) {
                        let props = one.child("table-cell-properties").map(kept_attrs);
                        cell_styles.push((name.to_string(), props, part));
                    }
                }
                _ => {}
            }
        }
    }
    let mut entries: Vec<Value> = Vec::new();
    let mut elements = 0usize;
    let mut covered = 0usize;
    let mut resolved = 0usize;
    let mut cell_elements = 0usize;
    let mut covered_cells = 0usize;
    let mut shade_cells = 0usize;
    let mut align_cells = 0usize;
    let mut lined_cells = 0usize;
    let mut padded_cells = 0usize;
    let mut cells_unresolved = 0usize;
    for (at, tbl) in text_body
        .descendants("table")
        .into_iter()
        .take(limit)
        .enumerate()
    {
        let style = crate::odsheet::attr_of(tbl, "style-name").map(String::from);
        let held = style
            .as_ref()
            .and_then(|want| tables_style.iter().find(|had| &had.0 == want));
        let mut cols: Vec<Value> = Vec::new();
        for kid in tbl.children.iter() {
            if kid.local() != "table-column" {
                continue;
            }
            elements += 1;
            let name = crate::odsheet::attr_of(kid, "style-name").map(String::from);
            let got = name
                .as_ref()
                .and_then(|want| columns.iter().find(|had| &had.0 == want));
            let repeated = crate::odsheet::attr_of(kid, "number-columns-repeated")
                .and_then(|raw| raw.trim().parse::<usize>().ok())
                .unwrap_or(1);
            covered += repeated;
            if got.is_some() {
                resolved += 1;
            }
            cols.push(json!({
                "written": kept_attrs(kid),
                "style": name,
                "style_part": got.map(|one| one.2),
                "repeated": repeated,
                "width": got.and_then(|one| one.1.clone()),
                "mm": got
                    .and_then(|one| crate::paper::length(one.1.as_ref()?["style:column-width"].as_str()?)),
            }));
        }
        // 格子：底色 / 边 / 垂直对齐没有「写在格子上」的地方，一律一跳在样式上；
        // `table:covered-table-cell` 是被合并掉的占位格，没有内容，所以另数一本。
        // 三个「有几格」的计数都含占位格（`covered_cells` 单列，减得回来）
        let mut cell_list: Vec<Value> = Vec::new();
        let mut my_cells = 0usize;
        let mut my_covered = 0usize;
        for (row, tr) in tbl
            .children
            .iter()
            .filter(|one| one.local() == "table-row")
            .enumerate()
        {
            for (col, tc) in tr
                .children
                .iter()
                .filter(|one| {
                    let local = one.local();
                    local == "table-cell" || local == "covered-table-cell"
                })
                .enumerate()
            {
                let is_covered = tc.local() == "covered-table-cell";
                my_cells += 1;
                covered_cells += usize::from(is_covered);
                my_covered += usize::from(is_covered);
                let cname = crate::odsheet::attr_of(tc, "style-name").map(String::from);
                let got = cname
                    .as_ref()
                    .and_then(|want| cell_styles.iter().find(|had| &had.0 == want));
                if cname.is_some() && got.is_none() {
                    cells_unresolved += 1;
                }
                let props = got.and_then(|one| one.1.clone());
                let cell_map = props.as_ref().and_then(|one| one.as_object());
                let shading = cell_map
                    .and_then(|had| had.get("fo:background-color"))
                    .and_then(|one| one.as_str().map(String::from));
                let valign = cell_map
                    .and_then(|had| had.get("style:vertical-align"))
                    .and_then(|one| one.as_str().map(String::from));
                let mut edges: serde_json::Map<String, Value> = serde_json::Map::new();
                let mut padded = false;
                if let Some(had) = cell_map {
                    for (key, value) in had {
                        if CELL_EDGES.iter().any(|one| *one == key.as_str()) {
                            let short = key.rsplit(':').next().unwrap_or(key.as_str());
                            edges.insert(short.to_string(), value.clone());
                        } else if key.starts_with("fo:padding") {
                            padded = true;
                        }
                    }
                }
                let lined = edges
                    .values()
                    .any(|value| value.as_str().map(edge_lined).unwrap_or(false));
                let has_edges = !edges.is_empty();
                shade_cells += usize::from(shading.is_some());
                align_cells += usize::from(valign.is_some());
                lined_cells += usize::from(lined);
                padded_cells += usize::from(padded);
                cell_list.push(json!({
                    "row": row,
                    "col": col,
                    "covered": is_covered,
                    "attrs": kept_attrs(tc),
                    "style": cname,
                    "style_part": got.map(|one| one.2),
                    "written": props.unwrap_or(Value::Null),
                    "shading": shading,
                    "valign": valign,
                    "borders": Value::Object(edges),
                    "borders_present": has_edges,
                    "lined": lined,
                    "padded": padded,
                }));
            }
        }
        cell_elements += my_cells;
        entries.push(json!({
            "at": at,
            "cell_elements": my_cells,
            "covered_cells": my_covered,
            "cells": cell_list.into_iter().take(limit).collect::<Vec<Value>>(),
            "name": crate::odsheet::attr_of(tbl, "name").map(String::from),
            "style": style,
            "style_part": held.map(|one| one.2),
            "written": held.and_then(|one| one.1.clone()).unwrap_or(Value::Null),
            "mm": held
                .and_then(|one| crate::paper::length(one.1.as_ref()?["style:width"].as_str()?)),
            "columns": cols.into_iter().take(limit).collect::<Vec<Value>>(),
        }));
    }
    json!({
        "tables": entries.len(),
        "column_elements": elements,
        "covered": covered,
        "resolved": resolved,
        "column_styles": columns.len(),
        "table_styles": tables_style.len(),
        "cell_styles": cell_styles.len(),
        "cell_styles_in_content": cell_styles
            .iter()
            .filter(|one| one.2 == "content.xml")
            .count(),
        "cell_styles_in_styles": cell_styles
            .iter()
            .filter(|one| one.2 == "styles.xml")
            .count(),
        "cell_elements": cell_elements,
        "covered_cells": covered_cells,
        "cells_unresolved": cells_unresolved,
        "shade_cells": shade_cells,
        "align_cells": align_cells,
        "lined_cells": lined_cells,
        "padded_cells": padded_cells,
        "list": entries,
    })
}

fn run_office_doc(app: &OfficeDoc, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("reading the document structure".to_string()),
    });
    let doc = open(&blob.bytes);
    let bytes = &blob.bytes[..];
    let mut notes: Vec<String> = Vec::new();
    let limit = crate::opack::take_limit(app.limit, LIMIT_DEFAULT);
    let result = if doc.family == Family::Ooxml && doc.app == "word" {
        let member = match zipread::member(bytes, "word/document.xml", DEFAULT_MEMBER_CAP) {
            Ok(one) => one,
            Err(why) => return Err(AppError::InvalidInput(why)),
        };
        if !member.verified {
            notes.push(format!("word/document.xml：{}", member.note));
        }
        let root = xmlscan::parse_str(&member.as_text());
        // `#doc` 的直接孩子是 `<w:document>`，`w:body` 在它下面一层：
        // 只往下走一步就会永远找不到 body，然后所有计数都从伪根走 ——
        // 数字照样对（descendants 是全树），但那条「没有 body」的假话会一直留在 notes 里。
        let document = root.child("document").unwrap_or(&root);
        let body = match document.child("body") {
            Some(one) => one,
            None => {
                notes.push("document.xml 里没有 w:body 元素".to_string());
                document
            }
        };
        let paragraphs = body.descendants("p");
        let mut headings: Vec<Value> = Vec::new();
        let mut styles: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        let mut empty = 0usize;
        let mut tally = Tally::default();
        for one in &paragraphs {
            let text = crate::office_text::paragraph_text(one);
            tally.add(&text);
            if text.is_empty() {
                empty += 1;
            }
            if let Some(style) = crate::office_text::style_of(one) {
                *styles.entry(style).or_insert(0) += 1;
            }
            if let Some(level) = crate::office_text::heading_of(one) {
                if headings.len() < limit {
                    headings.push(json!({"level": level, "text": text}));
                }
            }
        }
        let tables: Vec<Value> = body
            .descendants("tbl")
            .iter()
            .take(limit)
            .map(|one| {
                json!({
                    // 这两个数是 `descendants` 数的：一张嵌在格子里的表会让外面这张把
                    // 里面的行与格子一起算进来（那是「这份文件里有几个行/格标记」的问法）。
                    // 「这张表自己几行、每行几个格、哪格被合并了」在下面那份网格里，
                    // 它走的是直接孩子 —— 两本账口径不同，所以两个都交
                    "rows": one.descendants("tr").len(),
                    "cells": one.descendants("tc").len(),
                    "grid": crate::table_grid::ooxml(one, limit).to_json(),
                    "text": one.descendants("p").iter().filter(|p| !crate::office_text::paragraph_text(p).is_empty()).count(),
                })
            })
            .collect();
        let (rels, mut rel_notes) = relationships(bytes, &doc.entries);
        notes.append(&mut rel_notes);
        let hyperlinks: Vec<Value> = rels
            .iter()
            .filter(|one| one.kind == "hyperlink")
            .take(limit)
            .map(|one| json!({"target": one.target, "external": one.external, "resolves": one.resolved}))
            .collect();
        let image_parts: Vec<String> = rels
            .iter()
            .filter(|one| one.kind == "image")
            .filter_map(|one| one.resolved.clone())
            .take(limit)
            .collect();
        let count = |name: &str, part: &str| -> usize {
            // 部件不在包里就是零个：OOXML 的脚注 / 尾注 / 批注各自是一个部件，
            // 没写这个部件等于文档里没有这类东西。只有遗留 .doc 看不见它们，才给 null。
            zipread::member(bytes, part, DEFAULT_MEMBER_CAP)
                .map(|member| {
                    let root = xmlscan::parse_str(&member.as_text());
                    root.descendants(name)
                        .iter()
                        // 脚注与尾注部件里白坐着两条分隔符（separator 与
                        // continuationSeparator）：LibreOffice 与 Word 都写，
                        // 按元素个数数就会凭空多出两条「注」
                        .filter(|one| {
                            !matches!(
                                one.attr_local("type").unwrap_or_default(),
                                "separator" | "continuationSeparator"
                            )
                        })
                        .count()
                })
                .unwrap_or(0)
        };
        // 修订这份账。`word/settings.xml` 里的 `w:trackChanges` 说的是「往后还记不记」，
        // 与正文里已经存着的那些改动是两件事，所以两个都报
        let settings = zipread::member(bytes, "word/settings.xml", DEFAULT_MEMBER_CAP)
            .ok()
            .map(|one| xmlscan::parse_str(&one.as_text()));
        let revisions = crate::revise::docx_ledger(&paragraphs, settings.as_ref());
        // 保护与修订是两件事：一个是「这份文件让不让你改」，一个是「改过的那些痕迹」
        let protection = crate::protect::docx_document(settings.as_ref());
        // 编号那份账要跳两份件：定义在 `word/numbering.xml`，而段上没写的那些要看
        // `word/styles.xml` 里段点名的样式（Word 与 python-docx 都把 numPr 放在样式里）
        let has_numbering = doc
            .entries
            .iter()
            .any(|one| one.name == "word/numbering.xml");
        let numbering = zipread::member(bytes, "word/numbering.xml", DEFAULT_MEMBER_CAP)
            .ok()
            .map(|one| xmlscan::parse_str(&one.as_text()));
        let style_sheet = zipread::member(bytes, "word/styles.xml", DEFAULT_MEMBER_CAP)
            .ok()
            .map(|one| xmlscan::parse_str(&one.as_text()));
        // 图那一份账：一处号一次跳（`a:blip/@r:embed` → 这份件的关系表 → 部件），
        // 而「有没有替代文字」是单独一本账 —— 空的 descr 与没写不是一回事
        let pictures = docx_pictures(&body, &rels, limit);
        let with_alt = pictures
            .iter()
            .filter(|one| matches!(one["alt"]["descr"].as_str(), Some(raw) if !raw.is_empty()))
            .count();
        json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "wordprocessingml",
            "structure": {
                "paragraphs": paragraphs.len(),
                "empty_paragraphs": empty,
                "tables": body.descendants("tbl").len(),
                "table_rows": body.descendants("tr").len(),
                "table_cells": body.descendants("tc").len(),
                "sections": body.descendants("sectPr").len(),
                "breaks": body.descendants("br").len(),
                "page_breaks": body.descendants("br").iter().filter(|one| one.attr_local("type") == Some("page")).count(),
                "drawings": body.descendants("drawing").len(),
                "pictures": pictures.len(),
                "pictures_with_alt_text": with_alt,
                "pictures_without_alt_text": pictures.len() - with_alt,
                "picture_list": pictures,
                "text_boxes": body.descendants("txbxContent").len(),
                "insertions": body.descendants("ins").len(),
                "deletions": body.descendants("del").len(),
                "bookmarks": body.descendants("bookmarkStart").len(),
                // 这一族把段的对齐/缩进/段距写在段自己身上，分栏写在节上
                "paragraph_formats": docx_paragraph_formats(&paragraphs, limit),
                // 字符格式是另一本账：那一串字自己带一份 rPr，与段上那份不是一回事
                "run_formats": docx_run_formats(&paragraphs, style_sheet.as_ref(), bytes, limit),
                "columns": docx_columns(&body, limit),
                // 那一路要跳三跳，而且不从段上开始也走得通（样式里那份 numPr 也算）
                "numbering": docx_numbering(
                    &paragraphs,
                    numbering.as_ref(),
                    style_sheet.as_ref(),
                    has_numbering,
                    limit,
                ),
                // 这张表多宽有三本账：w:tblW、w:tblGrid、每一格的 w:tcW
                "table_layouts": docx_table_layouts(&body, limit),
                "fields": body.descendants("fldSimple").len() + body.descendants("fldChar").len(),
                // 部件存在 ≠ 文档用到了编号：notes.docx 带着 numbering.xml，
                // 正文里却一个 numPr 都没有（python-docx 没往里写列表）
                "has_numbering": body.descendants("numPr").len() > 0,
                "numbering_part": doc
                    .entries
                    .iter()
                    .any(|one| one.name == "word/numbering.xml"),
            },
            // 那张纸：一个 `w:sectPr` 一张（每节自己写尺寸与边距，单位 twips 换成 0.01mm）
            "page_setup": crate::paper::ledger(crate::paper::ooxml(body, limit)),
            "headings": headings,
            "styles": styles,
            "tables": tables,
            "images": image_parts,
            "hyperlinks": hyperlinks,
            "footnotes": count("footnote", "word/footnotes.xml"),
            "endnotes": count("endnote", "word/endnotes.xml"),
            "contents": docx_contents(&root),
            "comments": count("comment", "word/comments.xml"),
            "revisions": revisions.to_json(limit),
            "protection": protection,
            // 「多少字、多少页」这一问有两份账：自己数的与生产者自报的
            "statistics": {
                "ours": tally.to_json(),
                "producer": producer_counts(bytes),
            },
            "parts": doc.entries.iter().map(|one| one.name.clone()).filter(|one| one.starts_with("word/")).take(limit).collect::<Vec<String>>(),
            "notes": notes,
        })
    } else if doc.family == Family::Odf && doc.app == "word" {
        // ODF 文字：正文在 office:body > office:text，属性都带前缀而前缀是文件自己声明的，
        // 所以按局部名取（但要躲开 LibreOffice 抄的那份 calcext: 副本）。
        let member = match zipread::member(bytes, "content.xml", DEFAULT_MEMBER_CAP) {
            Ok(one) => one,
            Err(why) => return Err(AppError::InvalidInput(why)),
        };
        let href = |one: &xmlscan::Node| -> Option<String> {
            crate::odsheet::attr_of(one, "href").map(|one| one.to_string())
        };
        let root = xmlscan::parse_str(&member.as_text());
        let text_body = root
            .descendants("body")
            .into_iter()
            .find_map(|one| one.child("text"))
            .unwrap_or(&root);
        let mut paragraphs: Vec<&xmlscan::Node> = Vec::new();
        crate::office_text::odf_paragraphs(text_body, &mut paragraphs);
        let mut empty = 0usize;
        let mut tally = Tally::default();
        let mut styles: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for one in &paragraphs {
            let text = crate::office_text::odf_paragraph_text(one);
            tally.add(&text);
            if text.is_empty() {
                empty += 1;
            }
            if let Some(style) = crate::odsheet::attr_of(one, "style-name") {
                *styles.entry(style.to_string()).or_insert(0) += 1;
            }
        }
        // 标题在 ODF 里不是 `text:p` 而是 `text:h`：算字数要把它一起算，
        // 不然「这份文档多少字」会漏掉所有小标题
        for one in text_body.descendants("h") {
            tally.add(&crate::office_text::paragraph_text(one));
        }
        let headings: Vec<Value> = text_body
            .descendants("h")
            .iter()
            .take(limit)
            .map(|one| {
                json!({
                    "level": crate::odsheet::attr_of(one, "outline-level")
                        .and_then(|raw| raw.trim().parse::<u64>().ok()),
                    "text": crate::office_text::paragraph_text(one),
                })
            })
            .collect();
        let tables: Vec<Value> = text_body
            .descendants("table")
            .iter()
            .take(limit)
            .map(|one| {
                json!({
                    "name": crate::odsheet::attr_of(one, "name"),
                    // 与 OOXML 那一支同一条口径：这两个数走 `descendants`（嵌套表算进来），
                    // 网格走直接孩子，两本账都交
                    "rows": one.all("table-row").len(),
                    "cells": one.descendants("table-cell").len(),
                    "grid": crate::table_grid::odf(one, limit).to_json(),
                    "covered": one.descendants("covered-table-cell").len(),
                    "text": one
                        .descendants("p")
                        .iter()
                        .filter(|p| !crate::office_text::paragraph_text(p).is_empty())
                        .count(),
                })
            })
            .collect();
        // 脚注与尾注在 ODF 里是同一个 `text:note`，靠 note:class 分家
        let notes_found: Vec<&xmlscan::Node> = text_body.descendants("note");
        let of_class = |want: &str| -> usize {
            notes_found
                .iter()
                .filter(|one| crate::odsheet::attr_of(one, "note-class") == Some(want))
                .count()
        };
        let unclassed = notes_found.len() - of_class("footnote") - of_class("endnote");
        if unclassed > 0 {
            notes.push(format!(
                "{} 个 text:note 没写 note:class，分不清脚注还是尾注",
                unclassed
            ));
        }
        let hyperlinks: Vec<Value> = text_body
            .descendants("a")
            .iter()
            .take(limit)
            .map(|one| {
                json!({
                    "target": href(one),
                    "text": one.text().trim(),
                })
            })
            .collect();
        let images: Vec<String> = text_body
            .descendants("image")
            .iter()
            .filter_map(|one| href(one))
            .take(limit)
            .collect();
        let statistic = zipread::member(bytes, "meta.xml", DEFAULT_MEMBER_CAP)
            .ok()
            .map(|one| {
                let meta = xmlscan::parse_str(&one.as_text());
                let mut out = json!({});
                if let Some(node) = meta.descendants("document-statistic").first() {
                    for (key, value) in &node.attrs {
                        let local = key.rsplit(':').next().unwrap_or(key).to_string();
                        out[local] = json!(value);
                    }
                }
                out
            })
            .unwrap_or_else(|| json!({}));
        notes.push(
            "ODF 的段落口径与 OOXML 一致：表格里也算段；`structure.sections` 是 text:section（内容分块），\
             不是 Word 那种分页设置"
                .to_string(),
        );
        // ODF 的修订存在两处：`text:changed-region` 是账（谁、什么时候、哪一类），
        // 删掉的字在 region 里，插入的字在正文那两个标记之间
        let revisions = crate::revise::odt_ledger(text_body, &paragraphs);
        // ODF 的文档级保护不在 content.xml 里，在 settings.xml 的那几个 config-item 上
        let protection = match zipread::member(bytes, "settings.xml", DEFAULT_MEMBER_CAP) {
            Ok(member) => {
                let settings_root = xmlscan::parse_str(&member.as_text());
                crate::protect::odt_document(&settings_root)
            }
            Err(_) => json!({"items": {}, "protected": false, "part": false}),
        };
        // 那张纸也不在 content.xml 里：它在 styles.xml 的页布局上（`fo:page-width="21.59cm"`
        // 这种自带单位的串）。部件整个读不出就是「没看成」，交 null 并说明，不交一张空表
        // 那张纸与那份编号账都不在 content.xml 里：页布局与列表样式的定义都在
        // styles.xml（LibreOffice 的 ODF 导出把 `text:list-style` 全写在那边），
        // 所以这一份解析两边共用
        let styles_part = match zipread::member(bytes, "styles.xml", DEFAULT_MEMBER_CAP) {
            Ok(member) => Some(xmlscan::parse_str(&member.as_text())),
            Err(why) => {
                notes.push(format!("styles.xml 读不出，那份纸看不了：{why}"));
                None
            }
        };
        let page_setup = match styles_part.as_ref() {
            Some(one) => crate::paper::ledger(crate::paper::odf(one, limit)),
            None => Value::Null,
        };
        let pictures = odt_pictures(text_body, limit);
        let with_alt = pictures
            .iter()
            .filter(|one| {
                one["alt_written"].as_bool() == Some(true)
                    && matches!(one["alt"].as_str(), Some(raw) if !raw.is_empty())
            })
            .count();
        json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "opendocument-text",
            "structure": {
                "paragraphs": paragraphs.len(),
                "empty_paragraphs": empty,
                "tables": tables.len(),
                "table_rows": text_body.descendants("table-row").len(),
                "table_cells": text_body.descendants("table-cell").len(),
                "covered_cells": text_body.descendants("covered-table-cell").len(),
                "sections": text_body.descendants("section").len(),
                "breaks": text_body.descendants("line-break").len(),
                // 图那一份账：ODF 把尺寸写在 frame 上（自带单位的串）、地址写在
                // `draw:image/@xlink:href`（包内相对路径，没有关系表这一层）、
                // 替代文字搬到了孩子元素 `svg:desc` 里
                "pictures": pictures.len(),
                "pictures_with_alt_text": with_alt,
                "pictures_without_alt_text": pictures.len() - with_alt,
                "picture_list": pictures,
                // 换页在 ODF 里不写在正文里，写在段落样式上（四份件都这样）；
                // `text:soft-page-break` 是另一件事（渲染时落下的那一格），另给一个键
                "page_breaks": odf_page_breaks(&root, text_body),
                "soft_page_breaks": text_body.descendants("soft-page-break").len(),
                // 这一族两段都不写在正文里：格式在段点名的样式上，栏在内联区点名的区样式上
                "paragraph_formats": odf_paragraph_formats(&paragraphs, &root, limit),
                // 这一族的字符格式在 `text:span` 点名的样式里，而夹在 span 中间的字
                // 文件根本没给它们立一个元素 —— 第三种情况，不是「有元素而没格式」
                "run_formats": odt_run_formats(&paragraphs, &root, styles_part.as_ref(), limit),
                "columns": odf_columns(&root, limit),
                // 级别在这族是嵌套层数，定义经常坐在另一个部件里
                "numbering": odf_numbering(&paragraphs, text_body, &root, styles_part.as_ref(), limit),
                // 列宽在样式那一跳上，而且样式名是照表名拼的（生产者约定，交原样）
                "table_layouts": odf_table_layouts(text_body, &root, styles_part.as_ref(), limit),
                "drawings": text_body.descendants("frame").len(),
                "annotations": text_body.descendants("annotation").len(),
                "lists": text_body.descendants("list").len(),
                "list_styles": text_body.descendants("list-style").len(),
                "bookmarks": text_body.descendants("bookmark-start").len()
                    + text_body.descendants("bookmark").len(),
                "sequences": text_body.descendants("sequence-decl").len(),
                "tracked_changes": text_body.descendants("tracked-changes").len(),
                "hyperlinks": hyperlinks.len(),
                "images": images.len(),
            },
            "page_setup": page_setup,
            "headings": headings,
            "styles": styles,
            "tables": tables,
            "images": images,
            "hyperlinks": hyperlinks,
            "footnotes": of_class("footnote"),
            "endnotes": of_class("endnote"),
            "contents": odf_contents(&root),
            "comments": text_body.descendants("annotation").len(),
            "revisions": revisions.to_json(limit),
            "protection": protection,
            // 与 docx 那一份同一个形状：自己数的与生产者自报的并排
            // （ODF 的生产者账在 meta.xml 的 document-statistic，值全是字符串）
            "statistics": {
                "ours": tally.to_json(),
                "producer": statistic,
            },
            "parts": doc.entries.iter().map(|one| one.name.clone()).take(limit).collect::<Vec<String>>(),
            "notes": notes,
        })
    } else if doc.family == Family::Rtf {
        // RTF 不是包，是一条流：能给的是段（`\par` 切的）、注、图与嵌入对象、那张纸、
        // 标题、目录（域指令），样式与字体各交一份账；
        // 分节归属与批注还是不判，那些项给 null 而不是 0
        let one = crate::rtf::extract(bytes);
        let mut tally = Tally::default();
        for line in &one.lines {
            tally.add(line);
        }
        let footnotes = one
            .note_list
            .iter()
            .filter(|had| had["kind"] == json!("footnote"))
            .count();
        let endnotes = one
            .note_list
            .iter()
            .filter(|had| had["kind"] == json!("endnote"))
            .count();
        // 图那一份账：这一族把尺寸写在**三种单位**上（像素、twips 目标、缩放百分比），
        // 格式有两份凭据（`pngblip` 那个控制字与数据自己带的前八个字节），
        // 替代文字住在 `{\*\picprop}` 那格的 `wzDescription` 里
        let pictures = one
            .picture_rows
            .iter()
            .cloned()
            .take(limit)
            .collect::<Vec<Value>>();
        let with_alt = one
            .picture_rows
            .iter()
            .filter(|had| matches!(had["alt"].as_str(), Some(raw) if !raw.is_empty()))
            .count();
        let mut notes = one.notes.clone();
        notes.push(
            "RTF 这一支只报流里数得清的东西：段落按 par 控制字切，注按目标群分\
             （尾注靠群里的 ftnalt 判），表给行数与格子数 —— 那两个数就是 row 与 cell \
             这两个控制字的条数（嵌套表的 nestrow / nestcell 另给，不混进去）。\
             「几张表」要判行与行之间的段落边界，这条规则拿一张表与两张表的对照件试过：\
             单表对、两张数成一张，所以 tables 留 null。链接是从 field 群里前瞻读出来的\
             （指令里的 HYPERLINK 地址与 fldrslt 的显示文字，显示文字照旧留在正文里），\
             域的总条数另给 fields。字体名与样式名从 fonttbl / stylesheet 那两群里读 \
             —— 那两群照旧整群跳过（一个字不进正文），所以 skipped_destinations 不因为这个\
             改动而变，只是读之前里面的名字从来没被交出来过。样式表写了多少条、字体表列了\
             几个字体也各交一个数；非 ANSI 字符集又含非 ASCII 字节的字体名交回 null\
             （条目自己那个 fcharset 号说明为什么），\
             不交一个我们按 cp1252 解出来的乱码。标题看样式名：段属性里的 \\sN 到样式表里\
             查到名字，名字写成 heading N 的那一段才算标题，层级就是 N —— 四份 RTF 件的\
             这本账与同一批字的 docx 那份逐条实测一字不差。名字不合这个形状\
             （比如「摘要」这种自定义样式名）就一条也不报，不替文件认一个层级。\
             那张纸（多大、边距多少）交的是文档级写的那一串 paperw / paperh / marg*，\
             与 docx 的 twips、odt 的「21.59cm」换成同一个 0.01mm 整数；\
             某一节的覆写住在 sectx 群里，这一族不判分节归属，所以只交这一条。\
             目录看域指令：TOC 那条域的指令原文在文件里把开关写成两个反斜杠\
             （单个反斜杠会开出一个控制字），解掉多出来的那一个之后与 docx 的 instrText \
             逐字同一个形状，所以「收几级」两家共用一把读取器；OOXML 那两键\
             （docPartGallery 与 sdt 个数）这一族没有，不造假。\
             批注按 annotation 群数出来（作者与字在 office-text 那一条命令里逐条交），\
             那一群写的 atndate 两个样本都对不上同一批字的 docx 里的 w:date，\
             解不动就只交原样那一串、日期给 null。\
             修订与保护还是不判，那两项同样是 null \
             —— null 是「没看」或「判不住」，不是「这份文件没有」"
                .to_string(),
        );
        let mut style_tally: std::collections::BTreeMap<String, usize> =
            std::collections::BTreeMap::new();
        for had in &one.style_uses {
            let name = had["name"].as_str().unwrap_or_default().to_string();
            *style_tally.entry(name).or_insert(0) +=
                usize::try_from(had["count"].as_u64().unwrap_or(0)).unwrap_or(0);
        }
        let hyperlinks: Vec<Value> = one
            .links
            .iter()
            .take(limit)
            .map(|had| {
                let target = had["target"].as_str().unwrap_or_default();
                json!({
                    "target": target,
                    // 站外与站内按地址自己说：这一族没有关系表可查
                    "external": target.contains("://"),
                    "text": had["text"],
                })
            })
            .collect();
        json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "rtf",
            "structure": {
                "paragraphs": one.lines.len(),
                "empty_paragraphs": Value::Null,
                // 「几张表」判不住（见上面那条说明）；行数与格子数是控制字的条数
                "tables": Value::Null,
                "table_rows": one.table_rows,
                "table_cells": one.table_cells,
                "table_row_defines": one.table_row_defines,
                "table_cell_paras": one.table_cell_paras,
                "nested_table_rows": one.nested_table_rows,
                "nested_table_cells": one.nested_table_cells,
                // 「几节」不判：`\sect` 是**收尾**符，最后一节自己不带一个，
                // 数出来的那个数不等于节数（两份两节的件都只写 1 个）—— 只交写了几个
                "sections": Value::Null,
                "section_breaks": one.break_words[5],
                "breaks": Value::Null,
                // 换页在这一族有三种写法：`\page` 在这里换页、`\pagebb` / `\pbb` 是
                // 「这一段之前换页」。Word 那条 `w:br w:type="page"` 到了 LibreOffice 的
                // RTF 导出里就成了 `\pagebb`（三份件都这样，`\page` 一条都没有）。
                // 合起来才等于「这份文档有几处换页」，六个词的条数也一并交出去
                "page_breaks": one.break_words[2] + one.break_words[3] + one.break_words[4],
                "break_words": {
                    "par": one.break_words[0],
                    "line": one.break_words[1],
                    "page": one.break_words[2],
                    "pagebb": one.break_words[3],
                    "pbb": one.break_words[4],
                    "sect": one.break_words[5],
                },
                "drawings": Value::Null,
                "text_boxes": Value::Null,
                "bookmarks": Value::Null,
                // 域比链接多：页码与日期也是域，所以两个数分开交
                "fields": one.fields,
                // 这一族的编号账：段上的 `\ls` / `\ilvl` 与 listtable 那一份定义对上才算
                // 解到；`has_numbering` 与 `numbering_part` 仍交 null —— 那两问问的是
                // 「OPC 包里有没有那个部件」，而 RTF 整个就一条流，没有一个部件可问
                "numbering": rtf_numbering(&one.numbering, limit),
                "has_numbering": Value::Null,
                "numbering_part": Value::Null,
                "pictures": one.pictures,
                // 图那一份账在 RTF 里是第三种形状：尺寸写在**三种单位**上（`picw` 像素、
                // `picwgoal` twips、`picscalex` 百分比），格式有两份凭据（`pngblip` 那个
                // 控制字与数据自己带的前八个字节），替代文字搬到 `{\*\picprop}` 那格里。
                // 条数 `pictures` 是整条流数到底的，这份账本最多存 512 张再按 `--limit` 截
                "picture_list": pictures,
                "pictures_with_alt_text": with_alt,
                "pictures_without_alt_text": one.picture_rows.len() - with_alt,
                // 字符格式：这一族写在群头上，所以这份账本按「说过话的群」一条一条交
                "run_formats": rtf_run_formats(&one, limit),
                "embedded_objects": one.embedded_objects,
                "skipped_destinations": one.skipped_destinations,
                "note_destinations": one.note_destinations,
                "page_destinations": one.page_destinations,
                // 样式表写了多少条、字体表列了几个字体（用了几个样式看 styles）
                "style_definitions": one.styles.len(),
                "font_definitions": one.fonts.len(),
                // 作者那条 `{\*\atnauthor …}` 出现了几次：与下面 comments 那个数不等，
                // 就是文件自己少写了作者或注（两边各数各的，不替它对齐）
                "annotation_authors": one.annotation_authors,
            },
            // 那张纸在 RTF 里写在文档级的属性串上（`\paperw` 那一串），只有一条：
            // 某一节的覆写住在 `{\*\sectx …}` 里，而这一族不判分节归属
            "page_setup": crate::paper::ledger(vec![crate::paper::rtf(&one.paper_writes)]),
            // 标题：段的样式号在段属性里，号的名字在样式表里，名字写成 `heading N`
            // 才算 —— 层级就写在名字里，这一支与 docx / odt 那两本账同一个形状
            "headings": one.headings.iter().take(limit).cloned().collect::<Vec<Value>>(),
            "styles": json!(style_tally),
            "font_list": one.fonts.iter().take(limit).cloned().collect::<Vec<Value>>(),
            "tables": [],
            "images": [],
            "hyperlinks": hyperlinks,
            "footnotes": footnotes,
            "endnotes": endnotes,
            "contents": rtf_contents(&one.field_instructions),
            // 批注按 `{\*\annotation …}` 那一群数（作者与字见 office-text）
            "comments": one.annotations.len(),
            // RTF 的修订（\strip / \on 那些）与红线都不在这一版里
            "revisions": Value::Null,
            "protection": Value::Null,
            "statistics": {
                "ours": tally.to_json(),
                "producer": Value::Null,
            },
            "parts": [],
            "notes": notes,
        })
    } else if doc.family == Family::Compound && doc.app == "word" {
        // 遗留 .doc：piece 表能给出的就是字符数与那些结构标记，别装作看得见表格线
        let cfb = match doc.compound.as_ref() {
            Some(one) => one,
            None => return Err(AppError::InvalidInput("复合文档打不开".to_string())),
        };
        let body = crate::word::read(cfb, bytes).map_err(AppError::InvalidInput)?;
        let raw = &body.text;
        json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "word-binary",
            "structure": {
                "paragraphs": raw.matches('\r').count(),
                "table_cell_marks": raw.matches('\u{7}').count(),
                "field_marks": raw.matches('\u{13}').count(),
                "object_marks": raw.matches('\u{1}').count() + raw.matches('\u{2}').count(),
                "annotation_marks": raw.matches('\u{5}').count(),
                "page_break_marks": raw.matches('\u{C}').count(),
                "soft_breaks": raw.matches('\u{B}').count(),
                "cp_total": body.cp_total,
                "pieces": body.pieces.len(),
                "table_streams": body.table_stream,
            },
            // 纸多大、边距多少在 .doc 里也是表流的东西（节属性那条链），这里没看
            "page_setup": Value::Null,
            "headings": Value::Null,
            "styles": Value::Null,
            "tables": Value::Null,
            "images": [],
            // 域标记数已经报在 structure.fields 里；这里的链接目标在表流的另一段，本版本不解
            "hyperlinks": [],
            "footnotes": null,
            "endnotes": null,
            // 这一版不读 .doc 的目录，所以给 null —— 是「没看」，不是「这份文档没有目录」
            "contents": null,
            "comments": null,
            // 遗留 .doc 的修订在表流的 LVC/PAPX 那套结构里，piece 表给不出「谁改了什么」
            "revisions": null,
            "protection": null,
            "parts": cfb.stream_names(),
            "notes": concat_notes(&body.notes, "遗留 .doc 的段落样式、表格与图形在表流的其它记录里，本版本只数正文里的结构标记；修订那份账（谁、什么时候）也住在表流里，这里读不出，宁可给 null"),
        })
    } else {
        return Err(AppError::InvalidInput(format!(
            "{} 不是 Word 文档（识别为 {} / {}）；表格用 office-sheet，演示文稿用 office-slide",
            app.path.display(),
            doc.app,
            doc.format
        )));
    };
    ctx.done(result.clone(), start.elapsed().as_millis() as u64);
    Ok(result)
}

fn concat_notes(mine: &[String], extra: &str) -> Vec<String> {
    let mut out: Vec<String> = mine.to_vec();
    out.push(extra.to_string());
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(name: &str) -> Value {
        let app = OfficeDoc {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit: 100,
            max_bytes: 1 << 26,
        };
        let (tx, _rx) = mpsc::channel();
        run_office_doc(&app, &Context::new_test(tx)).expect("office-doc 应成功")
    }

    /// 段落的格式与分栏：docx 写在段自己身上，ODF 跳一跳在样式里，RTF 这一族不判归属
    #[test]
    fn paragraph_formats_and_columns_sit_where_each_family_put_them() {
        let hand = run("para.docx");
        let formats = &hand["structure"]["paragraph_formats"];
        assert_eq!(formats["checked"], 6, "{formats}");
        assert_eq!(formats["listed"], 4, "没有 pPr 的段不上榜：{formats}");
        assert_eq!(formats["with_alignment"], 3);
        assert_eq!(formats["with_indent"], 3);
        assert_eq!(formats["with_spacing"], 2);
        let first = &formats["list"][0];
        assert_eq!(first["index"], 0);
        assert_eq!(
            first["elements"],
            json!(["spacing", "ind", "jc"]),
            "按文件写的顺序交：{first}"
        );
        assert_eq!(first["alignment"], "both");
        assert_eq!(first["indent"], json!({"left": "1701", "firstLine": "480"}));
        assert_eq!(
            first["spacing"],
            json!({"before": "120", "after": "60", "line": "360", "lineRule": "auto"}),
            "{first}"
        );
        assert_eq!(first["chars_written"], json!(false));
        assert_eq!(first["style"], Value::Null, "这一族没点名样式");
        let chars = &formats["list"][2];
        assert_eq!(chars["index"], 3, "第三段整个没有 pPr，所以不在账里");
        assert_eq!(chars["alignment"], "center");
        assert_eq!(
            chars["indent"],
            json!({"left": "0", "leftChars": "200", "firstLineChars": "150"}),
            "{chars}"
        );
        assert_eq!(
            chars["chars_written"],
            json!(true),
            "缩进的第二种单位：按字数"
        );
        assert_eq!(chars["spacing"], Value::Null);
        let sect = &formats["list"][3];
        assert_eq!(
            sect["elements"],
            json!(["sectPr"]),
            "段里只挂着节属性：{sect}"
        );
        assert_eq!(sect["alignment"], Value::Null);

        let columns = &hand["structure"]["columns"];
        assert_eq!(columns["sections"], 2, "{columns}");
        assert_eq!(columns["written"], 2);
        assert_eq!(columns["multi"], 1, "只有一节说了要两栏：{columns}");
        assert_eq!(
            columns["list"][0]["written"],
            json!({"space": "720"}),
            "「一栏」的写法就是没有 num"
        );
        assert_eq!(columns["list"][0]["count"], Value::Null);
        assert_eq!(
            columns["list"][1]["written"],
            json!({"space": "425", "num": "2"})
        );
        assert_eq!(columns["list"][1]["count"], "2");

        let odt = run("para.odt");
        let theirs = &odt["structure"]["paragraph_formats"];
        assert_eq!(theirs["checked"], 5, "{theirs}");
        assert_eq!(
            theirs["listed"], 5,
            "这一族每段都点名一个样式，所以全都上榜"
        );
        assert_eq!(
            theirs["resolved"], 4,
            "点名 Standard 那一段的样式在 styles.xml，这一跳不通"
        );
        let p1 = &theirs["list"][0];
        assert_eq!(p1["style"], "P1", "{p1}");
        assert_eq!(p1["parent"], "Standard");
        assert_eq!(
            p1["alignment"], "justify",
            "同一件事 docx 叫 both、这一族叫 justify"
        );
        assert_eq!(p1["indent"]["fo:margin-left"], "3cm");
        assert_eq!(p1["written"]["fo:line-height"], "150%");
        let plain = &theirs["list"][2];
        assert_eq!(plain["style"], "Standard");
        assert_eq!(plain["resolved"], json!(false));
        assert_eq!(plain["written"], Value::Null);
        let bychars = &theirs["list"][3];
        assert_eq!(
            bychars["indent"]["loext:margin-left"], "2ic",
            "按字数的缩进换了 namespace：{bychars}"
        );
        assert_eq!(
            bychars["indent"]["fo:margin-left"],
            Value::Null,
            "这一段 fo 里没有左边距：{bychars}"
        );
        assert_eq!(bychars["written"]["fo:text-align"], "center");
        assert_eq!(bychars["alignment"], "center");
        let end = &theirs["list"][4];
        assert_eq!(
            end["written"],
            json!({"style:page-number": "auto"}),
            "{end}"
        );
        assert_eq!(
            end["indent"],
            json!({}),
            "两组都不挨着，交空表而不是不交：{end}"
        );
        assert_eq!(end["spacing"], json!({}));
        let odt_columns = &odt["structure"]["columns"];
        assert_eq!(odt_columns["sections"], 1, "{odt_columns}");
        assert_eq!(odt_columns["written"], 1);
        let section = &odt_columns["list"][0];
        assert_eq!(section["name"], "TextSection", "{section}");
        assert_eq!(section["style"], "Sect1");
        assert_eq!(section["dont_balance"], "true");
        assert_eq!(
            section["written"],
            json!({"fo:column-count": "2", "fo:column-gap": "0.751cm"}),
            "{section}"
        );
        assert_eq!(section["parts"].as_array().expect("是数组").len(), 2);
        assert_eq!(section["parts"][0]["style:rel-width"], "32767*");
        assert_eq!(section["parts"][1]["fo:start-indent"], "0.375cm");

        // 注是坐在正文段**里面**的（`text:p > text:note > text:note-body > text:p`），
        // 所以「这份文档有几段」有两个数：段不落进去数 4、全树数 7。
        // 这一族两份账都取 4 那一份清单（否则 `index` 与段账对不上），
        // 而生产者自己写在 meta.xml 的那个 7 并排在 `statistics.producer` 里交出去
        let endodt = run("notes-end.odt");
        assert_eq!(endodt["structure"]["paragraphs"], 4, "{endodt}");
        assert_eq!(
            endodt["structure"]["paragraph_formats"]["checked"], 4,
            "段格式那份账必须与段账用同一份段清单：{endodt}"
        );
        assert_eq!(
            endodt["statistics"]["producer"]["paragraph-count"], "7",
            "LibreOffice 数的是连注在内的 7，不替它改成 4：{endodt}"
        );

        // RTF 这一族不报：LibreOffice 的导出在每一段前面把样式的默认重发一遍
        // （`\pard\plain\s0…\sa200` 段段都有），「这一段自己写了什么」与样式分不开；
        // 分栏更是全文没有一个 `\cols`
        let rtf = run("para.rtf");
        assert!(
            rtf["structure"].get("paragraph_formats").is_none(),
            "RTF 不判归属"
        );
        assert!(
            rtf["structure"].get("columns").is_none(),
            "这一族的分栏没读"
        );
        let legacy = run("notes.doc");
        assert!(
            legacy["structure"].get("paragraph_formats").is_none(),
            "{legacy}"
        );
    }

    /// 这张表多宽：docx 三本账、ODF 一跳在列样式上，而 LibreOffice 重写会把「auto/0」换成实数
    ///
    /// 期望值全部来自 `office_reader.py` 的 `docx_table_layouts` / `odf_table_layouts`
    #[test]
    fn table_widths_are_three_books_that_do_not_agree_by_design() {
        let hand = run("tables.docx");
        let laid = &hand["structure"]["table_layouts"];
        assert_eq!(laid["listed"], 2, "{laid}");
        assert_eq!(laid["with_tblW"], 2, "两份都写了 w:tblW");
        assert_eq!(laid["auto"], 2, "写的都是 type=auto w=0：说了等于没说");
        assert_eq!(
            laid["grid_sum"], 30480,
            "四列 4320 twips 各换成 0.01mm 再相加"
        );
        let one = &laid["list"][0];
        assert_eq!(one["w"], "0");
        assert_eq!(one["kind"], "auto");
        assert_eq!(one["mm"], 0, "auto 那个 0 也照换，不替它当成没说");
        assert_eq!(one["align"], Value::Null, "这一族没写 w:jc");
        assert_eq!(one["layout"], Value::Null);
        assert_eq!(one["cell_mar"], json!(false));
        assert_eq!(one["rows"], 3);
        assert_eq!(one["cols"], 2);
        assert_eq!(one["grid"][0]["w"], "4320");
        assert_eq!(
            one["cells"][0]["written"],
            json!({"type": "dxa", "w": "4320"})
        );
        assert_eq!(one["cells"][0]["span"], Value::Null);

        // 横向合并那一格自己写的是两列之和（5760），而网格仍是三条 2880
        let merged = run("tables-merged.docx");
        let wide = &merged["structure"]["table_layouts"]["list"][0];
        assert_eq!(wide["cols"], 3, "{wide}");
        assert_eq!(wide["grid"][2]["w"], "2880");
        assert_eq!(
            wide["cells"][0]["written"]["w"], "5760",
            "合并格报的是自己那一格"
        );
        assert_eq!(wide["cells"][0]["span"], "2");
        assert_eq!(wide["cells"][1]["written"]["w"], "2880");
        assert_eq!(wide["cells"][1]["span"], Value::Null);

        // 同一个格式重写一次：auto/0 换成实数，还补了 jc / tblInd / tblLayout / tblCellMar
        let lo = run("tables-lo.docx");
        let again = &lo["structure"]["table_layouts"];
        assert_eq!(again["auto"], 0, "{again}");
        let one = &again["list"][0];
        assert_eq!(one["w"], "8640");
        assert_eq!(one["mm"], 15240, "8640 twips 换 0.01mm");
        assert_eq!(one["align"], "start");
        assert_eq!(one["layout"], "fixed");
        assert_eq!(one["cell_mar"], json!(true));
        assert_eq!(one["indent"], json!({"w": "108", "type": "dxa"}), "{one}");
        assert_eq!(one["grid"][0]["w"], "4320", "网格那本两家一字不差");

        let odt = run("tables.odt");
        let theirs = &odt["structure"]["table_layouts"];
        assert_eq!(theirs["tables"], 2, "{theirs}");
        assert_eq!(
            theirs["column_elements"], 2,
            "一条列元素顶两列，所以条数不是列数"
        );
        assert_eq!(theirs["covered"], 4);
        assert_eq!(theirs["resolved"], 2);
        assert_eq!(theirs["table_styles"], 2);
        let held = &theirs["list"][0];
        assert_eq!(held["name"], "表格1", "表名是文件自己写的");
        assert_eq!(
            held["style_part"], "content.xml",
            "这一族宽度的样式在 content.xml"
        );
        assert_eq!(held["mm"], 15240, "15.24cm 与 8640 twips 换成同一个数");
        assert_eq!(held["written"]["table:align"], "left");
        let col = &held["columns"][0];
        assert_eq!(col["repeated"], 2);
        assert_eq!(
            col["style"], "表格1.A",
            "列样式名是照表名拼的（生产者约定）"
        );
        assert_eq!(col["width"]["style:column-width"], "7.62cm", "{col}");
        assert_eq!(col["mm"], 7620, "与 docx 那本网格的 4320 twips 对得上");
        let merged = run("tables-merged.odt");
        assert_eq!(merged["structure"]["table_layouts"]["covered"], 5, "3 + 2");
        let col = &merged["structure"]["table_layouts"]["list"][0]["columns"][0];
        assert_eq!(col["repeated"], 3);
        assert_eq!(col["width"]["style:column-width"], "5.08cm", "{col}");
        assert_eq!(col["mm"], 5080, "与 docx 那本的 2880 twips 对得上");

        // 格子自己的三样：底色、边框、垂直对齐，各在一格上，另两格什么都没设
        let shaded = run("shaded.docx");
        let cells = &shaded["structure"]["table_layouts"];
        assert_eq!(cells["shade_cells"], 1, "{cells}");
        assert_eq!(cells["border_cells"], 1);
        assert_eq!(cells["align_cells"], 1);
        assert_eq!(
            cells["empty_border_cells"], 0,
            "这一族没底色可说：整个元素都不写"
        );
        let one = &cells["list"][0]["cells"][0];
        assert_eq!(
            one["shading"],
            json!({"val": "clear", "color": "auto", "fill": "FFFF00"}),
            "{one}"
        );
        assert_eq!(one["borders"], Value::Null);
        assert_eq!(one["borders_present"], json!(false));
        let two = &cells["list"][0]["cells"][1];
        assert_eq!(
            two["borders"],
            json!({"top": {"val": "double", "sz": "6", "space": "0", "color": "FF0000"}}),
            "{two}"
        );
        assert_eq!(two["borders_present"], json!(true));
        assert_eq!(cells["list"][0]["cells"][2]["valign"], "bottom");
        assert_eq!(
            cells["list"][0]["cells"][3]["borders_present"],
            json!(false)
        );
        assert_eq!(cells["list"][0]["cells"][3]["shading"], Value::Null);

        // 同一个格式重写：每一格都补一个 <w:tcBorders></w:tcBorders> ——
        // 「元素在而一条边都没有」与「元素整个不在」必须分得开，所以两个键各交各的
        let lo2 = run("shaded-lo.docx");
        let again = &lo2["structure"]["table_layouts"];
        assert_eq!(again["shade_cells"], 1, "{again}");
        assert_eq!(again["border_cells"], 1, "写了边的还是只有一格");
        assert_eq!(again["empty_border_cells"], 5, "另外五格是空元素");
        assert_eq!(again["align_cells"], 1);
        assert_eq!(again["list"][0]["cells"][3]["borders_present"], json!(true));
        assert_eq!(again["list"][0]["cells"][3]["borders"], json!({}));
        assert_eq!(
            again["list"][0]["cells"][0]["shading"],
            json!({"val": "clear", "color": "auto", "fill": "FFFF00"}),
            "底色与十六进制的大小写都没被动过，只换了属性顺序"
        );
    }

    /// ODF 格子的三样都在一跳之外的样式上，而且一格可以什么都没写
    ///
    /// 期望值全部来自 `office_reader.py` 的 `odf_table_layouts`（同一份件两边读一遍）
    #[test]
    fn odf_cells_say_the_same_three_things_from_a_hop_away() {
        let hand = run("shaded.odt");
        let laid = &hand["structure"]["table_layouts"];
        assert_eq!(laid["cell_styles"], 6, "{laid}");
        assert_eq!(
            laid["cell_styles_in_content"], 6,
            "格子样式全在 content.xml —— 与编号那一批搬到 styles.xml 正好相反"
        );
        assert_eq!(laid["cell_styles_in_styles"], 0);
        assert_eq!(laid["cell_elements"], 6);
        assert_eq!(laid["covered_cells"], 0, "这张表没有合并");
        assert_eq!(laid["cells_unresolved"], 0, "每一格点名的样式都找着了");
        assert_eq!(laid["shade_cells"], 1);
        assert_eq!(laid["align_cells"], 1);
        assert_eq!(laid["lined_cells"], 1, "只有那一格写了真实的一条边");
        assert_eq!(laid["padded_cells"], 6, "连内边距的默认值也六格全写");
        let one = &laid["list"][0]["cells"][0];
        assert_eq!(one["style"], "表格1.A1", "样式名是按地址拼的");
        assert_eq!(one["style_part"], "content.xml");
        assert_eq!(one["covered"], json!(false));
        assert_eq!(
            one["attrs"]["table:style-name"], "表格1.A1",
            "格子上只有点名，没有值"
        );
        assert_eq!(
            one["shading"], "#ffff00",
            "docx 那边是 w:fill=FFFF00：小写、带 # 都照文件交"
        );
        assert_eq!(one["borders"], json!({"border": "none"}));
        assert_eq!(one["borders_present"], json!(true));
        assert_eq!(one["lined"], json!(false), "写了边，但那条边是 none");
        assert_eq!(one["valign"], Value::Null);
        let two = &laid["list"][0]["cells"][1];
        assert_eq!(
            two["borders"],
            json!({
                "border-left": "none",
                "border-right": "none",
                "border-top": "2.25pt double #ff0000",
                "border-bottom": "none"
            }),
            "这一格把 shorthand 换成了四条方位各写一次"
        );
        assert_eq!(two["lined"], json!(true));
        assert_eq!(
            two["written"]["style:border-line-width-top"], "0.026cm 0.026cm 0.026cm",
            "双线是三根线，2.25pt 是合起来的宽度"
        );
        let three = &laid["list"][0]["cells"][2];
        assert_eq!(three["valign"], "bottom");
        assert_eq!(three["shading"], Value::Null);

        // 合并过的那张：占位格与格子跨几列各是各的账
        let merged = run("tables-merged.odt");
        let theirs = &merged["structure"]["table_layouts"];
        assert_eq!(theirs["cell_elements"], 10, "{theirs}");
        assert_eq!(theirs["covered_cells"], 2, "两个占位格，另数一本");
        assert_eq!(theirs["padded_cells"], 9, "占位格有一个连样式都没点名");
        let wide = &theirs["list"][0]["cells"][0];
        assert_eq!(wide["attrs"]["table:number-columns-spanned"], "2");
        let hole = &theirs["list"][0]["cells"][1];
        assert_eq!(hole["covered"], json!(true));
        assert_eq!(hole["attrs"], json!({}), "这一格连名字都没写");
        assert_eq!(hole["style"], Value::Null);
        assert_eq!(hole["written"], Value::Null);
        assert_eq!(hole["borders_present"], json!(false));
        assert_eq!(hole["padded"], json!(false));
        let tall = &theirs["list"][1]["cells"][0];
        assert_eq!(tall["attrs"]["table:number-rows-spanned"], "2");
        let hole2 = &theirs["list"][1]["cells"][2];
        assert_eq!(hole2["covered"], json!(true));
        assert_eq!(hole2["style"], "表格2.A1", "另一种占位格还留着样式名");
        assert_eq!(hole2["padded"], json!(true));

        // 什么都没设的那张：底色、边、对齐三本都是 0，而 padding 十格全在
        let plain = run("tables.odt");
        let none = &plain["structure"]["table_layouts"];
        assert_eq!(none["shade_cells"], 0, "{none}");
        assert_eq!(none["align_cells"], 0);
        assert_eq!(none["lined_cells"], 0);
        assert_eq!(none["cell_elements"], 10);
        assert_eq!(none["padded_cells"], 10);
        assert_eq!(none["cell_styles"], 2, "十格共用两份样式");
    }

    /// 列表与编号：docx 那一路要跳三跳（样式那条也算），ODF 的级别是嵌套、定义在另一个部件
    ///
    /// 期望值全部来自 `office_reader.py` 的 `docx_numbering` 与 `odf_numbering`
    #[test]
    fn numbering_comes_from_three_places_and_levels_are_counted_differently() {
        let hand = run("lists.docx");
        let num = &hand["structure"]["numbering"];
        assert_eq!(num["part"], json!(true), "{num}");
        assert_eq!(num["nums"], 9, "模板自带九条 w:num");
        assert_eq!(num["abstracts"], 9);
        assert_eq!(num["checked"], 7, "{num}");
        assert_eq!(num["listed"], 6, "那一段什么都没说，不上榜");
        assert_eq!(num["on_paragraph"], 3);
        assert_eq!(num["via_style"], 3, "三段是靠样式来的");
        assert_eq!(num["both"], 0);
        assert_eq!(num["unresolved"], 1, "点了一个不存在的 numId");
        assert_eq!(
            num["used"],
            json!(["5", "1", "3", "77"]),
            "按正文点名的顺序"
        );
        let one = &num["list"][0];
        assert_eq!(one["index"], 1);
        assert_eq!(one["from"], "style");
        assert_eq!(one["num_id"], "5", "段上什么都没写，是样式替它写的");
        assert_eq!(one["para_num_id"], Value::Null);
        assert_eq!(one["style_num_id"], "5");
        assert_eq!(one["ilvl"], Value::Null, "样式那份只写 numId，不写级别");
        assert_eq!(one["abstract"], "7", "numId 5 指的是 abstractNum 7");
        assert_eq!(
            one["level_found"],
            json!(false),
            "ilvl 没写就不替它挑那一级"
        );
        let deep = &num["list"][3];
        assert_eq!(deep["index"], 4);
        assert_eq!(deep["from"], "paragraph");
        assert_eq!(deep["abstract"], "5");
        assert_eq!(deep["level"]["written"]["numFmt"], "bullet");
        assert_eq!(
            deep["level"]["written"]["lvlText"], "\u{f0b7}",
            "圆点不是 • ，是 Symbol 字体的那个私用区码位"
        );
        assert_eq!(
            deep["level"]["written"]["pStyle"], "ListBullet3",
            "定义里反指样式"
        );
        assert_eq!(
            deep["level"]["indent"],
            json!({"left": "1080", "hanging": "360"}),
            "{deep}"
        );
        assert_eq!(deep["level"]["fonts"]["ascii"], "Symbol", "{deep}");
        // 那一份定义只有一级（multiLevelType=singleLevel），所以点第二级是点空的
        let missing = &num["list"][4];
        assert_eq!(missing["index"], 5);
        assert_eq!(missing["ilvl"], "1");
        assert_eq!(missing["abstract"], "5");
        assert_eq!(missing["level_found"], json!(false), "{missing}");
        assert_eq!(missing["level"], Value::Null);
        let dangling = &num["list"][5];
        assert_eq!(dangling["num_id"], "77");
        assert_eq!(dangling["resolved"], json!(false));
        assert_eq!(dangling["abstract"], Value::Null);
        let first = &num["definitions"][0];
        assert_eq!(first["num_id"], "1");
        assert_eq!(
            first["abstract"], "8",
            "号是分开编的：numId 1 → abstractNumId 8"
        );
        assert_eq!(first["written"]["multiLevelType"], "singleLevel", "{first}");
        assert_eq!(first["referenced"], json!(true));
        assert_eq!(
            num["definitions"][1]["referenced"],
            json!(false),
            "定义了但没人点"
        );
        assert_eq!(num["definitions"].as_array().expect("是数组").len(), 9);

        // 同一个格式重写一次：编号既搬到段上、也留在样式上，级别补齐了九级，
        // 而那个点空的 numId 77 被改写成 numId 0（numbering.xml 里同样没有 0 这一条）
        let lo = run("lists-lo.docx");
        let again = &lo["structure"]["numbering"];
        assert_eq!(again["nums"], 7, "{again}");
        assert_eq!(again["abstracts"], 7);
        assert_eq!(again["both"], 3, "样式写一份、段上也写一份");
        assert_eq!(again["on_paragraph"], 3);
        assert_eq!(again["via_style"], 0);
        assert_eq!(again["used"], json!(["4", "1", "3", "0"]));
        let one = &again["list"][0];
        assert_eq!(one["from"], "both");
        assert_eq!(one["num_id"], "4");
        assert_eq!(one["ilvl"], "0", "重写那一家把级别也写出来了");
        assert_eq!(one["level_found"], json!(true));
        assert_eq!(
            one["level"]["written"]["lvlJc"], "start",
            "对齐词与上一家不同"
        );
        assert_eq!(
            one["level"]["indent"],
            json!({"start": "360", "hanging": "360"}),
            "缩进换了属性名：start 而不是 left"
        );
        assert_eq!(
            again["list"][4]["level"]["written"]["lvlText"], "%2.",
            "第二级有了"
        );
        assert_eq!(again["list"][5]["num_id"], "0");
        assert_eq!(again["list"][5]["resolved"], json!(false));
        assert_eq!(
            again["definitions"][0]["written"],
            json!({}),
            "那一家 nsid / tmpl / multiLevelType 一个都不写"
        );
        assert_eq!(
            again["definitions"][0]["levels"]
                .as_array()
                .expect("是数组")
                .len(),
            9
        );

        let odt = run("lists.odt");
        let theirs = &odt["structure"]["numbering"];
        assert_eq!(theirs["lists"], 4, "{theirs}");
        assert_eq!(theirs["items"], 5);
        assert_eq!(theirs["checked"], 7);
        assert_eq!(theirs["listed"], 6);
        assert_eq!(theirs["in_list"], 5, "五段坐在 text:list-item 里");
        assert_eq!(theirs["max_depth"], 2, "第二级是套了一层 list 表达出来的");
        assert_eq!(theirs["styles"], 10);
        assert_eq!(theirs["in_content"], 0, "这一族一份定义都不在 content.xml");
        assert_eq!(theirs["in_styles"], 10);
        let one = &theirs["list"][0];
        assert_eq!(one["index"], 1);
        assert_eq!(one["style"], "P1");
        assert_eq!(one["style_part"], "content.xml");
        assert_eq!(one["list_style"], "WWNum5");
        assert_eq!(one["list_part"], "styles.xml", "那一跳跨部件");
        assert_eq!(one["depth"], 1);
        assert_eq!(one["chain"], json!(["WWNum5"]));
        assert_eq!(one["level"]["kind"], "list-level-style-number");
        assert_eq!(one["level"]["level"], "1", "这一族的级别从 1 数起");
        assert_eq!(
            one["level"]["written"]["style:num-format"], "1",
            "1 就是阿拉伯数字"
        );
        let deep = &theirs["list"][4];
        assert_eq!(deep["depth"], 2);
        assert_eq!(
            deep["chain"],
            json!(["WWNum3", null]),
            "套在里面那一层的 text:list 连样式名都不写"
        );
        assert_eq!(deep["level"]["level"], "2");
        let none = &theirs["list"][5];
        assert_eq!(none["index"], 6);
        assert_eq!(none["style"], "P4");
        assert_eq!(
            none["list_style"], "",
            "空串是文件自己写的：这一族用它说「不套列表」"
        );
        assert_eq!(none["depth"], 0);
        assert_eq!(none["list_part"], Value::Null);
        assert_eq!(none["resolved"], json!(false));
        let plain = run("notes.odt");
        assert_eq!(
            plain["structure"]["numbering"]["listed"], 0,
            "这份件一条列表都没用"
        );
        assert_eq!(
            plain["structure"]["numbering"]["styles"], 10,
            "但定义了十份"
        );
    }

    /// RTF 那一族的列表：号写在段上、定义在星号群里、那一句标签是生产者算好写进流的
    ///
    /// 期望值全部来自 `lyco_rtf.py` 的 `rtf_text`（同一份件两边各读一遍）
    #[test]
    fn rtf_lists_carry_their_numbers_in_the_stream() {
        let out = run("lists.rtf");
        let num = &out["structure"]["numbering"];
        assert_eq!(num["list_definitions"], 7, "{num}");
        assert_eq!(num["overrides"], 7);
        assert_eq!(num["levels"], 63, "七份 × 九级，全写出来");
        assert_eq!(num["checked"], 7, "段是 par 与 row 切的，空段也算");
        assert_eq!(num["listed"], 5);
        assert_eq!(num["with_ls"], 5);
        assert_eq!(num["label_words"], 5);
        assert_eq!(num["resolved"], 5);
        assert_eq!(
            num["override_list"][3],
            json!({"ls": "4", "list_id": "4", "override_count": "0"}),
            "号本自己说这一条覆写了零级"
        );
        let one = &num["list"][0];
        assert_eq!(one["ls"], "4");
        assert_eq!(one["list_id"], "4");
        assert_eq!(one["template_id"], "4");
        assert_eq!(one["level_found"], json!(true));
        assert_eq!(one["at"], 1, "段号是切出来的那一条，第 0 段是标题");
        assert_eq!(one["style_index"], 70);
        assert_eq!(one["style_name"], "List Number");
        assert_eq!(one["label_written"], "\\pard\\plain  1.\\tab");
        assert_eq!(one["label"], "1.");
        assert_eq!(one["label_tab"], json!(true));
        assert_eq!(one["indent"]["li"], "360");
        assert_eq!(one["level"]["at"], 0);
        assert_eq!(one["level"]["nfc"], "0");
        assert_eq!(one["level"]["level_text"], "\\'02\\'00.;");
        assert_eq!(one["level"]["indent"], "360");
        // 圆点那一级：定义里点 `\f1`（字体表里那一个就是 Symbol），
        // 而文件算出来的那一句标签自己点 `\f7` —— 两个号各按各的交，不替它对齐
        let dot = &num["list"][2];
        assert_eq!(dot["level"]["nfc"], "23");
        assert_eq!(dot["level"]["font"], "1");
        assert_eq!(dot["label_font"], "7");
        assert_eq!(dot["level"]["level_text"], "\\'01\\u-3913 ?;");
        assert_eq!(dot["label"], "\u{f0b7}");
        assert_eq!(dot["style_name"], "List Bullet");
        // 第二级：这一族九级都写着，所以点得到（同一批字的 docx 那边是 false）
        let deep = &num["list"][4];
        assert_eq!(deep["ilvl"], "1");
        assert_eq!(deep["level"]["at"], 1);
        assert_eq!(deep["level_found"], json!(true));
        assert_eq!(num["definitions"][0]["nfc"][0], "23");
        assert_eq!(num["definitions"][3]["nfc"][0], "0");
        assert_eq!(num["definitions"][6]["nfc"][0], "255");
        assert_eq!(num["definitions"][0]["used_by"], 1);
        assert_eq!(num["definitions"][2]["used_by"], 2);
        assert_eq!(num["definitions"][3]["used_by"], 2);

        // 带着整个模板的定义、一段也没用：这一族几乎每份件都这样
        let plain = run("notes.rtf");
        assert_eq!(plain["structure"]["numbering"]["list_definitions"], 7);
        assert_eq!(plain["structure"]["numbering"]["levels"], 63);
        assert_eq!(
            plain["structure"]["numbering"]["listed"], 0,
            "没有一段点过号"
        );
        let end = run("notes-end.rtf");
        assert_eq!(end["structure"]["numbering"]["list_definitions"], 1);
        assert_eq!(end["structure"]["numbering"]["levels"], 9);
        // 而 .doc 仍然不交这个键：那一份的列表在表流里
        let legacy = run("notes.doc");
        assert!(
            legacy["structure"].get("numbering").is_none(),
            ".doc 不交编号这份账"
        );
    }

    /// 「谁在什么时候改了哪一段」这一问。期望值全部来自 `lyco_revisions.py`，
    /// 而这份 fixture 是 LibreOffice 写的 OOXML：它把一次插入拆成两个 `w:ins`
    /// （数字与单位各一条），所以**元素数 6 与逻辑改动 4 不是一回事**，两个都要报
    #[test]
    fn the_revision_ledger_says_who_changed_what_where() {
        let out = run("revisions-lo.docx");
        let rev = &out["revisions"];
        assert_eq!(rev["changes_total"], 4, "{rev}");
        assert_eq!(rev["elements"]["insertions"], 3, "一次编辑被拆成两条");
        assert_eq!(rev["elements"]["deletions"], 2);
        assert_eq!(rev["elements"]["format_changes"], 1);
        assert_eq!(
            rev["paragraph_marks"], 0,
            "LibreOffice 导出时丢了段落标记那条"
        );
        assert_eq!(rev["track_changes"], json!(false), "文件自己说没开着记录");
        let changes = rev["changes"].as_array().expect("是数组");
        assert_eq!(
            changes[0],
            json!({
                "index": 0, "kind": "insertion", "author": "张三",
                "date": "2026-03-05T09:12:00Z", "paragraph": 2,
                "text": "124000 元", "elements": 2, "paragraph_mark": false,
            }),
            "第一处：{}",
            changes[0]
        );
        assert_eq!(changes[1]["kind"], "deletion");
        assert_eq!(changes[1]["author"], "李四");
        assert_eq!(changes[1]["text"], "89000 元");
        assert_eq!(changes[1]["elements"], 2);
        // 改格式这一条带着**被改了格式的那些字**：字没动，但「改了哪里的样子」就是这几个字。
        // 这几个字不在这个元素里（那装的是新的 rPr），在它所在的 run 身上 —— 不留这一步，
        // docx 这条永远是空的，而 ODF 那侧的 region 区间里有字，两份账就不平（CI 比出来的）
        assert_eq!(changes[2]["kind"], "format-change");
        assert_eq!(changes[2]["author"], "王五");
        assert_eq!(changes[2]["text"], "，请复核。");
        assert_eq!(changes[3]["text"], "整段是新加的。");
        assert_eq!(changes[3]["paragraph"], 2 + 1, "整段新加的是下一段");
        // 段落序号的基准与 structure.paragraphs 同一份列表：标题算第 0 段
        assert_eq!(out["structure"]["paragraphs"], 4);
        assert_eq!(out["structure"]["insertions"], 3);
        assert_eq!(out["structure"]["deletions"], 2);
        let note = rev["notes"]
            .as_array()
            .expect("有 notes")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(note.contains("6 个修订元素合成 4 条"), "{note}");
    }

    /// 同一条规则的另一种形状：python-docx 手写的那份没被拆开，而且段落标记自己
    /// 也算一处改动 —— 它与同段同作者同时间的正文那条**不许合并**
    #[test]
    fn a_paragraph_mark_revision_stays_apart_from_its_text() {
        let out = run("revisions.docx");
        let rev = &out["revisions"];
        assert_eq!(rev["changes_total"], 5, "{rev}");
        assert_eq!(rev["elements"]["insertions"], 3);
        assert_eq!(rev["elements"]["deletions"], 1);
        assert_eq!(rev["paragraph_marks"], 1);
        let marks: Vec<&Value> = rev["changes"]
            .as_array()
            .expect("是数组")
            .iter()
            .filter(|one| one["paragraph_mark"].as_bool() == Some(true))
            .collect();
        assert_eq!(marks.len(), 1, "{rev}");
        assert_eq!(marks[0]["text"], "", "段落标记自己没有正文");
        assert_eq!(marks[0]["author"], "张三");
        assert_eq!(marks[0]["paragraph"], 3);
        // 没被拆开，所以每条就是一个元素
        assert!(rev["changes"]
            .as_array()
            .expect("是数组")
            .iter()
            .all(|one| one["elements"] == 1));
        assert!(rev["notes"].as_array().expect("有 notes").is_empty());
    }

    /// 同一批改动在 ODF 里的样子：region 自己就是逻辑改动（LO 已经把拆开的那份合回去了），
    /// 所以 4 处与 OOXML 那边合成后的 4 条对上；日期少一个 `Z`、格式改动带着字，
    /// 是这两个格式自己的差别，不替文件统一
    #[test]
    fn the_same_revisions_read_from_the_opendocument_side() {
        let out = run("revisions.odt");
        let rev = &out["revisions"];
        assert_eq!(rev["changes_total"], 4, "{rev}");
        assert_eq!(rev["elements"]["insertions"], 2);
        assert_eq!(rev["elements"]["deletions"], 1);
        assert_eq!(rev["elements"]["format_changes"], 1);
        assert_eq!(rev["paragraph_marks"], 0);
        assert_eq!(rev["track_changes"], json!(false));
        let changes = rev["changes"].as_array().expect("是数组");
        assert_eq!(
            changes
                .iter()
                .map(|one| one["kind"].as_str().unwrap_or(""))
                .collect::<Vec<_>>(),
            vec!["insertion", "deletion", "format-change", "insertion"]
        );
        assert_eq!(changes[0]["text"], "124000 元", "插入的字在正文的区间里");
        assert_eq!(changes[0]["date"], "2026-03-05T09:12:00");
        assert_eq!(changes[1]["text"], "89000 元", "删掉的字在 region 里");
        assert_eq!(
            changes[2]["text"], "，请复核。",
            "ODF 的格式改动带着被改的字"
        );
        assert_eq!(changes[3]["text"], "整段是新加的。");
        assert_eq!(changes[0]["paragraph"], 1, "标题是 text:h，不占正文段的号");
        assert_eq!(changes[3]["paragraph"], 2);
        assert_eq!(
            out["structure"]["paragraphs"], 3,
            "region 里那份删掉的段不算正文"
        );
        let authors = rev["authors"].as_array().expect("是数组");
        assert_eq!(authors.len(), 3, "{authors:?}");
        assert_eq!(authors[0]["name"], "张三");
        assert_eq!(authors[0]["changes"], 2);
    }

    /// 「这份能动吗」：docx 的保护写在 `word/settings.xml`，与修订是两份账。
    /// 两份 fixture 都要读出来（一份 python-docx 手注入、一份 LibreOffice 重写）
    #[test]
    fn protection_says_which_kind_of_editing_is_restricted() {
        for name in ["protected.docx", "protected-lo.docx"] {
            let out = run(name);
            let one = &out["protection"];
            assert_eq!(one["element"], json!(true), "{name}: {one}");
            assert_eq!(one["protected"], json!(true), "{name}");
            assert_eq!(one["edit"], json!("readOnly"), "{name}");
            assert_eq!(one["password"], json!(true), "有 hash 就是设了口令：{name}");
            assert_eq!(one["algorithm"], json!("typeAny"), "{name}");
            assert_eq!(one["spin_count"], json!("100000"), "{name}");
        }
        assert_eq!(run("notes.docx")["protection"]["element"], json!(false));
    }

    /// ODF 那一侧：文档级保护在 `settings.xml` 的 config-item 上，而 docx 的编辑限制
    /// **不会**跟着转换过来（这条是 LibreOffice 实测，见 fixture README）
    #[test]
    fn the_odt_side_does_not_carry_a_docx_restriction() {
        let out = run("protected.odt");
        let one = &out["protection"];
        assert_eq!(one["protected"], json!(false), "{one}");
        assert_eq!(one["items"]["ProtectForm"], json!(false));
        assert_eq!(one["items"]["ProtectBookmarks"], json!(false));
        assert_eq!(one["items"]["ProtectFields"], json!(false));
        assert_eq!(run("notes.odt")["protection"]["protected"], json!(false));
        // 遗留 .doc 读不出：给 null，不假称「没保护」
        assert!(run("notes.doc")["protection"].is_null(), "读不出就说读不出");
    }

    /// LibreOffice 从 RTF 导入写出的那份脚注样本：`word/footnotes.xml` 里有四条
    /// `w:footnote`，其中两条是 `separator` / `continuationSeparator` —— 它们是
    /// 排版用的占位，不是文档里的注（期望值来自 `office_reader.py` 的 docx_facts）
    #[test]
    fn footnote_separators_are_not_counted_as_footnotes() {
        let out = run("notes-foot.docx");
        assert_eq!(out["footnotes"], 2, "{out}");
        assert_eq!(out["endnotes"], 0, "这份文件根本没有 endnotes.xml");
        assert_eq!(out["comments"], 0);
        assert_eq!(out["structure"]["paragraphs"], 4, "注的字不在正文里");
    }

    /// 尾注那一条分支第一次有真件可走。这份是 LibreOffice 的 docx 导出器写的
    /// （Writer 自己没有尾注概念，RTF 的 `\endnote` 在导入时就被摊进正文，
    /// 所以只有「注进去再让它照抄」这条路）：部件里三条 `w:endnote`，
    /// 它自己写的两条分隔符是 `<w:separator/>` / `<w:continuationSeparator/>` 那种写法。
    /// 期望值来自 `office_reader.py` 的 docx_facts
    #[test]
    fn endnotes_are_counted_from_a_part_the_producer_wrote() {
        let out = run("notes-end.docx");
        assert_eq!(out["endnotes"], json!(1), "{out}");
        assert_eq!(out["footnotes"], json!(2), "同一份文件里脚注仍占两条");
        assert_eq!(out["comments"], json!(0));
        assert_eq!(out["structure"]["paragraphs"], 4, "尾注的字不算正文");
        assert!(
            out["parts"]
                .as_array()
                .expect("parts 是数组")
                .iter()
                .any(|one| one == "word/endnotes.xml"),
            "{:?}",
            out["parts"]
        );
        // 同一件事在遗留 .doc 那一支只能说「看不见」：注住在表流的另一段，给 null
        assert!(
            run("notes.doc")["endnotes"].is_null(),
            ".doc 看不见就交回 null"
        );
        // ODF 那一支也第一次有真尾注：LibreOffice 的 ODT 导出器写
        // `text:note-class="endnote"`（编号还换成罗马数字），与 OOXML 那份同一笔账
        let odt = run("notes-end.odt");
        assert_eq!(odt["kind"], "opendocument-text", "{odt}");
        assert_eq!(odt["endnotes"], json!(1), "{odt}");
        assert_eq!(odt["footnotes"], json!(2), "同一份 ODT 里脚注仍占两条");
    }

    /// 「这份文档有没有目录、收了几级」的第一批真件。三家存法根本不同：
    /// OOXML 把级别写在域指令的文字里（`TOC \o "1-2" \h`，外面套一层
    /// `w:sdt` + `docPartGallery="Table of Contents"`），ODF 写在
    /// `text:table-of-content-source` 的 `outline-level` 属性上，RTF 也写在域指令里
    /// 但那个开关要成对写反斜杠 —— 所以各报各的形状。
    /// 三份件都是 LibreOffice 写的（目录注进 docx 让它照抄，rtf / odt 是它的导出，
    /// 见 office_fixtures.py）
    /// （期望值来自 `office_reader.py` 的 `docx_contents()` / `odf_contents()` 与 `lyco_rtf.py`）
    #[test]
    fn a_table_of_contents_is_reported_whichever_way_the_file_keeps_it() {
        let doc = run("toc.docx");
        let contents = &doc["contents"];
        assert_eq!(contents["present"], json!(true), "{contents}");
        assert_eq!(contents["via"], json!("doc-part-gallery"), "{contents}");
        assert_eq!(
            contents["levels"],
            json!("1-2"),
            "几级写在域指令里：{contents}"
        );
        assert_eq!(contents["sdt"], json!(1), "{contents}");
        assert_eq!(
            contents["fields"],
            json!(["TOC \\o \"1-2\" \\h"]),
            "域指令原文照文件写的交（引号是 LO 写成 &quot; 的那对）：{contents}"
        );
        let odt = run("toc.odt");
        let got = &odt["contents"];
        assert_eq!(got["present"], json!(true), "{got}");
        assert_eq!(got["names"], json!(["目录1"]), "{got}");
        assert_eq!(
            got["outline_level"],
            json!("2"),
            "ODF 的级别在属性上：{got}"
        );
        assert_eq!(got["title"], json!("目录"), "{got}");
        assert_eq!(
            got["entry_templates"],
            json!(10),
            "LO 十级模板都写出来，不管用不用得上：{got}"
        );
        // 第三种写法：RTF 的目录就是流里的一条域，指令原文里的开关成对写反斜杠
        // （`{ TOC \\o "1-2" \\h}`）。解掉多出来的那一个，这一串与上面 docx 的
        // `w:instrText` 逐字相同 —— 所以两家共用一把「几级」读取器，不是巧合而是同一份规范
        let rtf = run("toc.rtf");
        let got = &rtf["contents"];
        assert_eq!(got["present"], json!(true), "{got}");
        assert_eq!(got["via"], json!("field"), "{got}");
        assert_eq!(
            got["fields"],
            json!(["TOC \\o \"1-2\" \\h"]),
            "双反斜杠要解掉一个：{got}"
        );
        assert_eq!(got["levels"], json!("1-2"), "{got}");
        assert_eq!(
            got["levels"], contents["levels"],
            "同一份文档的两种写法，级数得是同一个字：docx {contents} vs rtf {got}"
        );
        assert!(
            got.get("galleries").is_none() && got.get("sdt").is_none(),
            "OOXML 专属的两键不许造：{got}"
        );
        // 反面对照：没目录的件报 present=false（键在、值为假），而不是 null
        assert_eq!(run("notes.docx")["contents"]["present"], json!(false));
        assert_eq!(run("notes.odt")["contents"]["present"], json!(false));
        assert_eq!(
            run("notes.rtf")["contents"]["present"],
            json!(false),
            "那条域是 HYPERLINK，不是 TOC：不算目录"
        );
        assert!(run("notes.doc")["contents"].is_null(), ".doc 没看就给 null");
    }

    /// 换页在 ODF 里是**段落样式上**的一个属性（`fo:break-before="page"`），正文里
    /// 没有任何换页元素。以前这一条数的是 `text:soft-page-break`（渲染时落下的那一格），
    /// 于是四份明明换了页的件全报 0。两家读者现在都走「段落的 style-name → 那个样式的
    /// paragraph-properties」这两跳；父样式链上也可能写，但手上没有那种样本，不跟那条链
    /// （期望值来自 `office_reader.py` 的 `odf_page_breaks()`）
    #[test]
    fn a_page_break_in_odf_sits_on_the_paragraph_style() {
        for name in ["notes.odt", "toc.odt", "notes-hf.odt", "protected.odt"] {
            assert_eq!(run(name)["structure"]["page_breaks"], 1, "{name}");
        }
        for name in ["comments.odt", "paper-a4.odt", "tables.odt"] {
            assert_eq!(run(name)["structure"]["page_breaks"], 0, "{name}");
        }
        // 「作者要的换页」与「渲染时落下的那一格」是两件事，两个键各自交
        assert_eq!(run("notes.odt")["structure"]["soft_page_breaks"], 0);
        // 同一批字在另两家的写法不同，数出来是同一个 1
        assert_eq!(run("notes.docx")["structure"]["page_breaks"], 1);
        assert_eq!(run("notes.rtf")["structure"]["page_breaks"], 1);
    }

    /// RTF 也终于有这一问了：它不是包，是一条流 —— 数得清的是段（par 切的行）、
    /// 注（按目标群，尾注靠群里的 ftnalt）、图与嵌入对象、那张纸、目录（看域指令）；
    /// 样式名与表格线不判，那些项交回 null（「没看」）而不是 0（「没有」）
    /// （期望值来自 `lyco_rtf.py` 的 rtf_text）
    #[test]
    fn rtf_answers_structure_with_only_what_the_stream_proves() {
        let out = run("notes-end.rtf");
        assert_eq!(out["kind"], "rtf", "{out}");
        assert_eq!(out["structure"]["paragraphs"], 4, "{out}");
        assert_eq!(out["footnotes"], 2, "两条脚注：{out}");
        assert_eq!(out["endnotes"], 1, "一条尾注，靠 ftnalt 判：{out}");
        assert_eq!(out["structure"]["note_destinations"], 3, "{out}");
        assert_eq!(out["structure"]["pictures"], 0, "{out}");
        assert!(out["structure"]["tables"].is_null(), "没看就是 null：{out}");
        // 样式不再交 null：那一群里写着什么就用什么（样式表自己写了 16 条）
        assert_eq!(
            out["styles"],
            json!({"Normal": 4}),
            "正文里只用了 Normal：{out}"
        );
        assert_eq!(out["structure"]["style_definitions"], 16, "{out}");
        assert_eq!(out["structure"]["font_definitions"], 9, "{out}");
        // 目录这一问 RTF 也答得出：那一条流里一个域都没有（`\field` 条数为零），
        // 所以 present=false（不是 null —— 看过了，只是没有）
        assert_eq!(out["contents"]["present"], json!(false), "{out}");
        assert_eq!(out["contents"]["fields"], json!([]), "{out}");
        assert!(out["contents"]["levels"].is_null(), "{out}");
        // 批注也答得出：那一条流里没有 annotation 群，两条列表都是空的
        // （不是 null —— 这一支看过整条流了）
        assert_eq!(out["comments"], json!(0), "{out}");
        assert_eq!(out["structure"]["annotation_authors"], 0, "{out}");
        assert_eq!(
            out["statistics"]["ours"],
            json!({"characters": 116, "characters_no_spaces": 100, "words_by_space": 20}),
            "{out}"
        );
        // 另一份带一张图的：注是零条，图数得出来，表的行与格子也数得出来
        let pic = run("notes.rtf");
        assert_eq!(pic["structure"]["pictures"], 1, "{pic}");
        // 那一份流里有一条批注（作者 liuqi），另一份两份注的在 office_text 那边逐条对
        assert_eq!(pic["comments"], json!(1), "{pic}");
        assert_eq!(pic["structure"]["annotation_authors"], 1, "{pic}");
        assert_eq!(pic["structure"]["paragraphs"], 7, "{pic}");
        assert_eq!(pic["footnotes"], 0);
        assert_eq!(pic["endnotes"], 0);
        assert_eq!(pic["structure"]["table_rows"], 2, "{pic}");
        assert_eq!(pic["structure"]["table_cells"], 4, "{pic}");
        assert_eq!(pic["structure"]["table_row_defines"], 2, "{pic}");
        assert_eq!(pic["structure"]["table_cell_paras"], 4, "{pic}");
        // 页眉页脚那份：口袋数也要交，注仍是零，表也仍是零
        let hf = run("notes-hf.rtf");
        assert_eq!(hf["structure"]["page_destinations"], 6, "{hf}");
        assert_eq!(hf["structure"]["paragraphs"], 3, "{hf}");
        assert_eq!(hf["structure"]["table_rows"], 0, "{hf}");
        assert_eq!(hf["structure"]["table_cells"], 0, "{hf}");
        assert_eq!(hf["structure"]["fields"], 0, "{hf}");
        // 标题：段属性里的 \sN 到样式表里查到 `heading N` 才算层级。
        // 期望值来自 lyco_rtf.py，与同一批字的 docx 那本账同一个形状
        assert_eq!(
            hf["headings"],
            json!([{"level": 1, "text": "带页眉的一页"}]),
            "页眉那份也认这一条：{hf}"
        );
        // 链接：那一个 field 群里的 HYPERLINK 地址与显示文字，
        // 显示文字照旧算正文的一行（少这一条就会「读到链接、丢了字」）
        let links = run("notes.rtf");
        assert_eq!(
            links["hyperlinks"][0]["target"], "https://example.com/budget",
            "{links}"
        );
        assert_eq!(links["hyperlinks"][0]["external"], json!(true), "{links}");
        assert_eq!(links["hyperlinks"][0]["text"], "预算制度", "{links}");
        assert_eq!(links["structure"]["fields"], 1, "{links}");
        assert!(
            links["notes"]
                .as_array()
                .expect("有说明")
                .iter()
                .any(|one| one.as_str().unwrap_or_default().contains("fldrslt")),
            "说明里要讲清链接是从哪读的：{links}"
        );
        // 同一批字的两种存法：地址必须一模一样
        assert_eq!(
            links["hyperlinks"][0]["target"],
            run("notes.docx")["hyperlinks"][0]["target"],
            "同一批字的 docx 与 rtf 两份链接账"
        );
        // 标题：段属性里的 \sN 到样式表里查到 `heading N` 才算层级。
        // 期望值来自 lyco_rtf.py，与同一批字的 docx 那本账同一个形状
        assert_eq!(
            links["headings"],
            json!([
                {"level": 1, "text": "一级标题：预算口径"},
                {"level": 2, "text": "二级标题：明细"}
            ]),
            "{links}"
        );
        assert_eq!(
            links["headings"],
            run("notes.docx")["headings"],
            "同一批字的 docx 与 rtf 两份标题账"
        );
        // 一条标题都没有的那份：空数组，不是 null（这一支确实看过样式表）
        assert_eq!(
            out["headings"],
            json!([]),
            "notes-end.rtf 全用 Normal，就该交空：{out}"
        );
        assert!(
            out["notes"]
                .as_array()
                .expect("是数组")
                .iter()
                .map(|one| one.as_str().unwrap_or_default())
                .any(|one| one.contains("heading N")),
            "说明里要讲清层级是从样式名来的：{out}"
        );
    }

    /// RTF 的表：行数与格子数是控制字的条数（同一份文档的 docx 与 odt 两副账给一样的数），
    /// 「几张表」却判不住 —— 那条把连续的 row 定义算成一张表的规则，在一份单表件上对、
    /// 在一份两张表（3×2 与 2×2）的件上把两张数成一张，所以 tables 交回 null 并写明理由。
    /// 期望值来自 `lyco_rtf.py` 的 rtf_text 与 `office_reader.py` 的 docx_facts / odt_facts
    #[test]
    fn rtf_counts_table_rows_and_cells_but_not_tables() {
        let out = run("tables.rtf");
        assert_eq!(out["kind"], "rtf", "{out}");
        assert_eq!(out["structure"]["paragraphs"], 10, "{out}");
        assert_eq!(out["structure"]["table_rows"], 5, "五个 row 结束符：{out}");
        assert_eq!(out["structure"]["table_cells"], 10, "十个 cell：{out}");
        assert_eq!(
            out["structure"]["table_row_defines"], 5,
            "五个 trowd：{out}"
        );
        assert_eq!(
            out["structure"]["table_cell_paras"], 10,
            "十个 intbl：{out}"
        );
        assert_eq!(out["structure"]["nested_table_rows"], 0, "{out}");
        assert_eq!(out["structure"]["nested_table_cells"], 0, "{out}");
        assert!(
            out["structure"]["tables"].is_null(),
            "几张表判不住，就留 null：{out}"
        );
        // 同一批字的另两副账：那一份是敢报表数的
        for name in ["tables.docx", "tables.odt"] {
            let other = run(name);
            assert_eq!(other["structure"]["tables"], 2, "{name}：{other}");
            assert_eq!(other["structure"]["table_rows"], 5, "{name}");
            assert_eq!(other["structure"]["table_cells"], 10, "{name}");
        }
        // 标题也在这三份件里对得上：rtf 的层级来自样式名，docx 的来自 w:pStyle，
        // 同一批字的两本账必须一字不差
        assert_eq!(
            out["headings"],
            json!([
                {"level": 1, "text": "两张表的样本"},
                {"level": 2, "text": "二级标题"}
            ]),
            "{out}"
        );
        assert_eq!(
            out["headings"],
            run("tables.docx")["headings"],
            "rtf 与 docx 两份标题账"
        );
    }

    /// 那张纸：三家各写各的单位（docx 与 RTF 写 twips，odt 写「21.59cm」这种带单位的串），
    /// 换成 0.01mm 的整数之后要给同一个数。期望值来自 `lyco_pages.py` 的独立读取。
    /// `notes-hf` 那一份留着生产者自己的不一致：docx 上下写 1440 twips，
    /// LibreOffice 的 odt 与 rtf 两个导出都写 720 / 1.27cm —— 三个数都摆出来，不挑一个当准
    #[test]
    fn the_same_paper_comes_out_of_three_spellings() {
        // 只比这四边：页眉页脚的距离与 book 边的距离只有 OOXML 写在这一项上，
        // 另两家根本没有这两个口袋（整份对象一比就是假红）
        fn four(one: &Value) -> Vec<Value> {
            let m = &one["page_setup"]["papers"][0]["margins"];
            vec![
                m["top"].clone(),
                m["right"].clone(),
                m["bottom"].clone(),
                m["left"].clone(),
            ]
        }
        for stem in ["notes", "notes-end", "tables"] {
            let got: Vec<Value> = vec![
                run(&format!("{stem}.docx")),
                run(&format!("{stem}.odt")),
                run(&format!("{stem}.rtf")),
            ];
            for one in &got {
                assert_eq!(one["page_setup"]["unit"], "0.01mm", "{stem}：{one}");
                let paper = &one["page_setup"]["papers"][0];
                assert_eq!(paper["width"], 21590, "{stem}：{paper}");
                assert_eq!(paper["height"], 27940, "{stem}：{paper}");
            }
            assert_eq!(four(&got[0]), four(&got[1]), "{stem}");
            assert_eq!(four(&got[0]), four(&got[2]), "{stem}");
        }
        let dx = run("notes.docx");
        let paper = &dx["page_setup"]["papers"][0];
        assert_eq!(paper["margins"]["top"], 2540, "{paper}");
        assert_eq!(
            paper["margins"]["header"], 1270,
            "页眉的距离只有 OOXML 写在这一项上：{paper}"
        );
        assert_eq!(
            paper["margins"]["gutter"], 0,
            "写 0 就交 0，不是 null：{paper}"
        );
        assert!(
            paper["orient"].is_null(),
            "竖排时 docx 干脆不写这一项：{paper}"
        );
        assert_eq!(paper["written"]["unit"], "twips", "{paper}");
        assert_eq!(
            paper["written"]["width"], "12240",
            "文件自己那串也要交：{paper}"
        );
        assert_eq!(paper["written"]["margins"]["top"], "1440", "{paper}");
        let od = run("notes.odt");
        let paper = &od["page_setup"]["papers"][0];
        assert_eq!(paper["orient"], "portrait", "odt 会明写方向：{paper}");
        assert!(
            paper["margins"]["header"].is_null(),
            "odt 的页眉距离不在这项上：{paper}"
        );
        assert!(paper["written"]["unit"].is_null(), "单位已在串里：{paper}");
        assert_eq!(paper["written"]["width"], "21.59cm", "{paper}");
        // 两节的那一份：一节一条纸，两条都交
        let hf = run("notes-hf.docx");
        assert_eq!(
            hf["page_setup"]["papers"].as_array().expect("是数组").len(),
            2,
            "{hf}"
        );
        assert_eq!(hf["page_setup"]["papers"][1]["width"], 21590, "{hf}");
        // 同一份件的另两家：上下边距被 LibreOffice 换成了 0.5 英寸（这是文件写的）
        assert_eq!(
            hf["page_setup"]["papers"][0]["margins"]["top"], 2540,
            "docx 自己写 1440 twips"
        );
        assert_eq!(
            run("notes-hf.odt")["page_setup"]["papers"][0]["margins"]["top"],
            1270,
            "odt 写 1.27cm"
        );
        assert_eq!(
            run("notes-hf.rtf")["page_setup"]["papers"][0]["margins"]["top"],
            1270,
            "rtf 写 margt720"
        );
        // 遗留 .doc：节属性在表流里，这一版没看，所以是 null 而不是空表
        assert!(
            run("notes.doc")["page_setup"].is_null(),
            "没看就交 null，别交一个看起来像「没有」的空表"
        );
        // 第二份尺寸（A4 + 一节横排）。这里有两个要钉的东西：
        // 1) 换算不是凑上 Letter 的 —— 三家的文档默认那一份换成 0.01mm 后仍是同一个数，
        //    而那个数是 21001×29700，不是整数意义上的 21000×29700（OOXML 与 RTF 把 A4 的
        //    短边写作 11906 twips，LibreOffice 的 ODF 又照抄成 `21.001cm`）——
        //    所以这一支不给尺寸起名：「这就是 A4」那种查表在三份件上都会落空；
        // 2) 横排那一节 docx 与 odt 都写着（`orient=landscape`、宽高对调），
        //    而 LibreOffice 的 RTF 导出整份文件一个 `\landscape` 都没有 ——
        //    那一条流里就只剩文档默认的纵向。文件的差不是读者的差，照实交
        let a4docx = run("paper-a4.docx");
        let a4odt = run("paper-a4.odt");
        let a4rtf = run("paper-a4.rtf");
        assert_eq!(
            [
                a4docx["page_setup"]["papers"][0]["width"].clone(),
                a4odt["page_setup"]["papers"][0]["width"].clone(),
                a4rtf["page_setup"]["papers"][0]["width"].clone(),
            ],
            [json!(21001), json!(21001), json!(21001)],
            "短边三家同一个数：{a4docx}"
        );
        assert_eq!(
            a4docx["page_setup"]["papers"][0]["height"], 29700,
            "{a4docx}"
        );
        assert_eq!(
            a4docx["page_setup"]["papers"][0]["written"]["width"], "11906",
            "文件自己写的是 twips：{a4docx}"
        );
        assert_eq!(
            a4odt["page_setup"]["papers"][0]["written"]["width"], "21.001cm",
            "odt 那一份照抄了同一个长度：{a4odt}"
        );
        assert_eq!(
            a4docx["page_setup"]["papers"]
                .as_array()
                .expect("是数组")
                .len(),
            2,
            "{a4docx}"
        );
        let land = &a4docx["page_setup"]["papers"][1];
        assert_eq!(land["width"], 29700, "{land}");
        assert_eq!(land["height"], 21001, "宽高对调：{land}");
        assert_eq!(land["orient"], "landscape", "第一次有真件走到这里：{land}");
        assert_eq!(
            land["margins"]["top"], 1499,
            "1.5 厘米 → 1499（0.01mm 的下一位舍掉）：{land}"
        );
        assert_eq!(
            a4odt["page_setup"]["papers"][1]["orient"], "landscape",
            "{a4odt}"
        );
        assert_eq!(a4odt["page_setup"]["papers"][1]["width"], 29700, "{a4odt}");
        assert_eq!(
            a4rtf["page_setup"]["papers"]
                .as_array()
                .expect("是数组")
                .len(),
            1,
            "RTF 那一条流里只有文档默认：{a4rtf}"
        );
        assert!(
            a4rtf["page_setup"]["papers"][0]["orient"].is_null(),
            "整份 RTF 一个 landscape 都没写，那就交 null：{a4rtf}"
        );
    }

    /// 结构数字要与独立读者算出来的逐项一致（期望值：office_reader.py 的 docx_facts）
    #[test]
    fn counts_a_real_docx_structure() {
        let out = run("notes.docx");
        assert_eq!(out["kind"], "wordprocessingml");
        let s = &out["structure"];
        assert_eq!(s["paragraphs"], 11, "{s}");
        assert_eq!(s["empty_paragraphs"], 2, "{s}");
        assert_eq!(s["tables"], 1);
        assert_eq!(s["table_rows"], 2);
        assert_eq!(s["table_cells"], 4);
        assert_eq!(s["sections"], 1);
        assert_eq!(s["drawings"], 1, "那张 Pillow 画的 PNG");
        assert_eq!(s["page_breaks"], 1);
        assert_eq!(s["has_numbering"], json!(false), "正文里没用编号：{s}");
        assert_eq!(s["numbering_part"], json!(true), "包里有编号部件：{s}");
        assert_eq!(
            out["headings"],
            json!([
                {"level": 1, "text": "一级标题：预算口径"},
                {"level": 2, "text": "二级标题：明细"}
            ]),
            "{out}"
        );
        assert_eq!(out["styles"]["Heading1"], 1);
        assert_eq!(out["styles"]["Heading2"], 1);
        assert_eq!(out["comments"], json!(1), "批注部件要一起数");
        assert_eq!(out["footnotes"], json!(0));
        assert_eq!(out["endnotes"], json!(0));
        // 「多少字」两份账：自己数的与生产者自报的。python-docx 写了 app.xml 却
        // 一个数都没数（Words/Characters 全是 0），所以那一份只能照抄不能当答案
        assert_eq!(
            out["statistics"]["ours"],
            json!({"characters": 67, "characters_no_spaces": 66, "words_by_space": 10}),
            "{out}"
        );
        assert_eq!(out["statistics"]["producer"]["words"], 0);
        assert_eq!(out["statistics"]["producer"]["pages"], 1);
        assert_eq!(out["images"], json!(["word/media/image1.png"]));
        assert_eq!(out["hyperlinks"][0]["target"], "https://example.com/budget");
        assert_eq!(out["hyperlinks"][0]["external"], json!(true));
        assert!(
            out["notes"].as_array().expect("有 notes").is_empty(),
            "{out}"
        );
    }

    /// ODT 的结构账：段落口径与 OOXML 一致（表格里也算段），但**批注里的段不算** ——
    /// ODF 的 `text:annotation` 嵌在正文段里面，LibreOffice 自报 10 段正是因为它把
    /// 批注里那一段也数了进去；标题层级在 `text:outline-level`，
    /// 脚注与尾注共用一个 `text:note`（这份两个都没写所以是 0）。
    /// 期望值全部来自 `odt_structure()`。
    #[test]
    fn an_opendocument_text_document_is_accounted_for() {
        let out = run("notes.odt");
        assert_eq!(out["kind"], "opendocument-text");
        assert_eq!(
            out["structure"]["paragraphs"], 9,
            "正文段不含批注里那一段：{out}"
        );
        assert_eq!(out["structure"]["empty_paragraphs"], 2, "{out}");
        assert_eq!(
            out["statistics"]["producer"]["paragraph-count"], "10",
            "生产者的 10 = 我们的 9 + 批注里那一段：口径差要在两边都说得清"
        );
        // 字符数两边完全一致（我们与 LibreOffice 各数各的），词数不一致是口径：
        // 它按中文词切（61），我们只按空白切（10）—— 所以键名叫 words_by_space
        assert_eq!(
            out["statistics"]["ours"],
            json!({"characters": 67, "characters_no_spaces": 66, "words_by_space": 10}),
            "{out}"
        );
        assert_eq!(out["statistics"]["producer"]["character-count"], "67");
        assert_eq!(out["statistics"]["producer"]["word-count"], "61");
        // 同一批字在 docx 与 odt 两边数出来必须一样（这份 odt 就是从那份 docx 转的）
        assert_eq!(
            out["statistics"]["ours"],
            run("notes.docx")["statistics"]["ours"]
        );
        assert_eq!(out["styles"]["Standard"], 8, "{out}");
        assert_eq!(out["styles"]["P2"], 1);
        assert_eq!(
            out["headings"],
            json!([
                {"level": 1, "text": "一级标题：预算口径"},
                {"level": 2, "text": "二级标题：明细"}
            ]),
            "{out}"
        );
        assert_eq!(out["structure"]["tables"], 1);
        assert_eq!(out["structure"]["table_rows"], 2);
        assert_eq!(out["structure"]["table_cells"], 4);
        assert_eq!(out["tables"][0]["name"], "表格1", "{out}");
        assert_eq!(out["comments"], 1, "text:annotation 就是批注");
        assert_eq!(out["footnotes"], 0, "一个 text:note 都没有");
        assert_eq!(out["endnotes"], 0);
        assert_eq!(out["hyperlinks"][0]["target"], "https://example.com/budget");
        assert_eq!(out["hyperlinks"][0]["text"], "预算制度");
        assert_eq!(out["structure"]["drawings"], 1, "那张图包在 draw:frame 里");
        assert_eq!(
            out["images"][0], "Pictures/1000000100000008000000088E4DF5D4.png",
            "{out}"
        );
        assert_eq!(out["structure"]["sequences"], 5, "五个页码/章节变量声明");
        assert_eq!(out["structure"]["tracked_changes"], 0);
        assert_eq!(
            out["statistics"]["producer"]["paragraph-count"], "10",
            "{out}"
        );
        assert_eq!(out["statistics"]["producer"]["page-count"], "2");
    }

    /// 表格里的段落也算段落：这是 Word 自己的口径，换了口径数字就对不上
    #[test]
    fn table_paragraphs_stay_paragraphs() {
        let out = run("notes.docx");
        assert!(
            out["structure"]["paragraphs"].as_u64().expect("有数") > 8,
            "{out}"
        );
        assert_eq!(out["tables"][0]["rows"], 2);
        assert_eq!(out["tables"][0]["cells"], 4);
    }

    /// 遗留 .doc 只报它真看得见的东西，并说明另一些为什么没有
    #[test]
    fn a_legacy_doc_reports_only_what_the_piece_table_shows() {
        let out = run("notes.doc");
        assert_eq!(out["kind"], "word-binary");
        // 段落数 = 正文里的硬回车数：独立读者对着 piece 表还原出的 139 个字符数到 10 个 \r
        assert_eq!(out["structure"]["paragraphs"], 10, "{out}");
        assert_eq!(out["structure"]["cp_total"], 139);
        assert_eq!(out["structure"]["pieces"], 1);
        assert_eq!(
            out["structure"]["table_cell_marks"], 6,
            "两个 2x2 表 + 行列分隔：{out}"
        );
        assert_eq!(out["structure"]["field_marks"], 1, "那个超链接域");
        assert!(
            out["headings"].is_null(),
            "样式表在表流里，本版本不解 → 不假装报得出来"
        );
        let note = out["notes"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one.as_str().unwrap_or(""))
            .collect::<Vec<_>>()
            .join(" ");
        assert!(note.contains("结构标记"), "{note}");
    }

    /// 不是 Word 文件要指路，而不是给一份空结构
    #[test]
    fn a_non_word_file_is_pointed_at_the_right_command() {
        let app = OfficeDoc {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/office/deck.pptx"),
            limit: 10,
            max_bytes: 1 << 20,
        };
        let (tx, _rx) = mpsc::channel();
        let why = run_office_doc(&app, &Context::new_test(tx)).unwrap_err();
        let text = why.to_string();
        assert!(text.contains("office-slide"), "{text}");
    }

    /// 字符样式：段上只写一个号，另一半话在另一份部件里；ODF 的 span 会套 span
    #[test]
    fn a_character_style_says_only_half_the_sentence() {
        let plain = run("charstyles.docx");
        let lo = run("charstyles-lo.docx");
        let odt = run("charstyles.odt");
        let rtf = run("charstyles.rtf");
        let had = &plain["structure"]["run_formats"];
        assert_eq!(had["checked"], 7, "{had}");
        assert_eq!(had["with_style"], 3);
        assert_eq!(had["style_found"], 3);
        assert_eq!(had["bold_on"], 1, "段上自己说粗的只有一串：{had}");
        assert_eq!(had["bold_from_style"], 1, "另一串是样式说的：{had}");
        assert_eq!(had["italic_from_style"], 2);
        assert_eq!(
            had["where_both_spoke"], 0,
            "没有一个开关被两处各说一次：{had}"
        );
        assert_eq!(lo["structure"]["run_formats"]["props_empty"], 4);
        assert_eq!(lo["structure"]["run_formats"]["style_found"], 3);
        let strong = &had["list"][1];
        assert_eq!(strong["style"], "Strong");
        assert_eq!(strong["style_name"], "Strong");
        assert_eq!(strong["style_parent"], "DefaultParagraphFont");
        assert_eq!(
            strong["elements"],
            json!(["rStyle"]),
            "段上只有号：{strong}"
        );
        assert_eq!(
            strong["switches"]["bold"],
            Value::Null,
            "段上什么都没说，不能说它不粗：{strong}"
        );
        assert_eq!(strong["style_switches"]["bold"], json!(true));
        assert_eq!(
            strong["style_format"],
            json!([
                {"element": "b", "written": {}},
                {"element": "bCs", "written": {}}
            ])
        );
        let both = &had["list"][3];
        assert_eq!(both["elements"], json!(["rStyle", "b"]));
        assert_eq!(both["switches"]["bold"], json!(true));
        assert_eq!(both["switches"]["italic"], Value::Null);
        assert_eq!(both["style_switches"]["italic"], json!(true));
        let subtle = &had["list"][5];
        assert_eq!(subtle["style"], "SubtleEmphasis");
        assert_eq!(
            subtle["style_name"], "Subtle Emphasis",
            "号与名不是一回事：{subtle}"
        );
        assert_eq!(
            subtle["style_format"][2]["written"],
            json!({"val": "808080", "themeColor": "text1", "themeTint": "7F"}),
            "主题色按写的交（解它要开 theme1.xml，这一族不开）：{subtle}"
        );
        let odt_ledger = &odt["structure"]["run_formats"];
        assert_eq!(odt_ledger["checked"], 8, "{odt_ledger}");
        assert_eq!(odt_ledger["spans"], 4);
        assert_eq!(
            odt_ledger["nested_spans"], 1,
            "span 会套 span：{odt_ledger}"
        );
        assert_eq!(
            odt_ledger["bold_on"], 2,
            "这一族的开关全从样式来，与 OOXML 那个 1 不可比：{odt_ledger}"
        );
        assert_eq!(odt_ledger["list"][1]["display"], "Strong Emphasis");
        assert_eq!(odt_ledger["list"][1]["found_in"], "styles");
        assert_eq!(odt_ledger["list"][1]["text"], "强调的字");
        assert_eq!(
            odt_ledger["list"][3]["text"], "",
            "外层那条的字由里层交，同一句不报两次"
        );
        assert_eq!(odt_ledger["list"][3]["depth"], 1);
        assert_eq!(odt_ledger["list"][4]["depth"], 2);
        assert_eq!(odt_ledger["list"][4]["found_in"], "content");
        let rtf_ledger = &rtf["structure"]["run_formats"];
        assert_eq!(rtf_ledger["checked"], 3, "{rtf_ledger}");
        assert_eq!(
            rtf_ledger["list"][0]["values"]["character_style"]["index"],
            "34"
        );
        assert_eq!(
            rtf_ledger["list"][0]["values"]["character_style"]["resolved"],
            json!(true)
        );
        assert_eq!(
            rtf_ledger["list"][0]["values"]["character_style"]["name"], "Strong",
            "定义写在星号群里，名字仍然要解得出来"
        );
        assert_eq!(
            rtf_ledger["list"][2]["values"]["character_style"]["name"],
            "Subtle Emphasis"
        );
    }

    /// 字符格式：三家把同一句话说在三个地方，而「明确不粗」与「没写」是两种话
    /// （期望值逐条来自 `office_reader.py` 的 `docx_run_formats` / `odf_run_formats`
    /// 与 `lyco_rtf.py` 的 `run_rows`，三份读者整份对过）
    #[test]
    fn character_formatting_is_per_run_and_spells_off_four_ways() {
        let plain = run("styled-text.docx");
        let lo = run("styled-text-lo.docx");
        let odt = run("styled-text.odt");
        let rtf = run("styled-text.rtf");
        for one in [&plain, &lo, &odt, &rtf] {
            let had = &one["structure"]["run_formats"];
            assert_eq!(had["bold_on"], 3, "三串字是粗的：{had}");
            assert_eq!(had["bold_off"], 1, "还有一串写的是「明确不粗」：{had}");
            assert_eq!(had["italic_on"], 3);
            assert_eq!(had["with_format"], 14, "十四串字自己说过话：{had}");
        }
        // 两家的「有没有这一格」正好一边一种，合成一个布尔就看不见这条差别
        assert_eq!(plain["structure"]["run_formats"]["with_props"], 14);
        assert_eq!(plain["structure"]["run_formats"]["props_empty"], 0);
        assert_eq!(lo["structure"]["run_formats"]["with_props"], 30);
        assert_eq!(
            lo["structure"]["run_formats"]["props_empty"], 16,
            "LibreOffice 重写时给每一串都补一个空的 rPr"
        );
        // 「明确不」的四家拼法：元素在而里面是 0 / false、样式里是 normal、群里是 \\b0
        let off = &plain["structure"]["run_formats"]["list"][20];
        assert_eq!(off["elements"], json!(["b"]));
        assert_eq!(
            off["switches"]["bold"],
            json!(false),
            "有 b 这个孩子而它说的是不：{off}"
        );
        assert_eq!(
            lo["structure"]["run_formats"]["list"][20]["format"][0]["written"]["val"],
            "false"
        );
        assert_eq!(
            odt["structure"]["run_formats"]["list"][20]["written"]["fo:font-weight"],
            "normal"
        );
        assert_eq!(
            odt["structure"]["run_formats"]["list"][20]["switches"]["bold"],
            json!(false)
        );
        let rtf_off = &rtf["structure"]["run_formats"]["list"][9];
        assert_eq!(
            rtf_off["format"][0]["digits"], "0",
            "否定写在数字参数上：{rtf_off}"
        );
        assert_eq!(rtf_off["switches"]["bold"], json!(false));
        // 那一个红：docx 直接写颜色，ODF 写小写带号，RTF 只写一个号 —— 跳一次文件自己的表
        assert_eq!(
            plain["structure"]["run_formats"]["list"][12]["values"]["color"],
            "C00000"
        );
        assert_eq!(
            odt["structure"]["run_formats"]["list"][12]["written"]["fo:color"],
            "#c00000"
        );
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][5]["values"]["color"]["index"],
            "23"
        );
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][5]["values"]["color"]["rgb"],
            "C00000"
        );
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][5]["values"]["color"]["resolved"],
            json!(true)
        );
        // 9 磅：两家是同一个半磅数，ODF 是自带单位的串，三个都交原样
        assert_eq!(
            plain["structure"]["run_formats"]["list"][16]["values"]["size"],
            "18"
        );
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][7]["values"]["size"],
            "18"
        );
        assert_eq!(
            odt["structure"]["run_formats"]["list"][16]["written"]["fo:font-size"],
            "9pt"
        );
        // 一串字里两个孩子：顺序是文件的，不重排（RTF 的群头写的是 i 在前）
        assert_eq!(
            plain["structure"]["run_formats"]["list"][24]["elements"],
            json!(["b", "i"])
        );
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][11]["format"][0]["element"],
            "i"
        );
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][11]["switches"]["bold"],
            json!(true)
        );
        // 一段里三种字：中间那一串在 docx 有自己的元素而里面是空的，
        // 在 python-docx 那份里连元素都没有，在 ODF 更是根本没有元素（`#text` 那一行）
        let middle = &plain["structure"]["run_formats"]["list"][27];
        assert_eq!(middle["text"], "这一串什么都不点。");
        assert_eq!(middle["props_written"], json!(false));
        assert_eq!(middle["elements"], json!([]));
        assert_eq!(
            lo["structure"]["run_formats"]["list"][27]["props_written"],
            json!(true)
        );
        assert_eq!(
            lo["structure"]["run_formats"]["list"][27]["elements"],
            json!([])
        );
        assert_eq!(
            odt["structure"]["run_formats"]["list"][27]["element"],
            "#text"
        );
        assert_eq!(
            odt["structure"]["run_formats"]["list"][27]["resolved"],
            Value::Null,
            "没有号可查，与查不到是两件事"
        );
        assert_eq!(odt["structure"]["run_formats"]["list"][26]["style"], "T1");
        assert_eq!(
            odt["structure"]["run_formats"]["list"][26]["found_in"],
            "content"
        );
        // RTF 没有「一串字」这个元素：这一问在这里判不住，而段前缀那些字另记一条数
        assert_eq!(rtf["structure"]["run_formats"]["with_props"], Value::Null);
        assert_eq!(rtf["structure"]["run_formats"]["props_empty"], Value::Null);
        assert_eq!(rtf["structure"]["run_formats"]["checked"], 14);
        assert_eq!(rtf["structure"]["run_formats"]["words_outside_groups"], 135);
        // 下划线这一族写在「日文下划线」那个口袋上：词先交出来
        assert_eq!(
            plain["structure"]["run_formats"]["list"][6]["format"][0]["element"],
            "u"
        );
        assert_eq!(
            plain["structure"]["run_formats"]["list"][6]["format"][0]["written"]["val"],
            "single"
        );
        assert_eq!(
            odt["structure"]["run_formats"]["list"][6]["written"]["style:text-underline-style"],
            "solid"
        );
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][2]["values"]["underline_word"],
            "aul"
        );
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][2]["switches"]["underline"],
            json!(true)
        );
        // 换字体那一句：RTF 那一个号在字体表里查得到条目，而名字解不动就还是 null
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][8]["values"]["asian_font"]["index"],
            "9"
        );
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][8]["values"]["asian_font"]["resolved"],
            json!(true)
        );
        assert_eq!(
            rtf["structure"]["run_formats"]["list"][8]["values"]["asian_font"]["name"],
            Value::Null
        );
        assert_eq!(
            plain["structure"]["run_formats"]["list"][18]["values"]["fonts"],
            json!({"ascii": "宋体", "hAnsi": "宋体"})
        );
    }

    /// 文档里那张图的账：尺寸两处、替代文字两处、锁两处，而三家把同一件事
    /// 写在三个不同的地方。期望值来自 `office_reader.py` 的
    /// `docx_picture_rows` / `odt_picture_rows`，两份读者逐字段整份对过
    #[test]
    fn a_picture_has_two_sizes_two_alts_and_two_locks() {
        let plain = run("images.docx");
        let rewritten = run("images-lo.docx");
        let floating = run("images-float.docx");
        let odt = run("images.odt");
        let list = |one: &Value| one["structure"]["picture_list"][0].clone();
        assert_eq!(plain["structure"]["pictures"], 1, "{plain}");
        assert_eq!(plain["structure"]["pictures_with_alt_text"], 1);
        assert_eq!(plain["structure"]["pictures_without_alt_text"], 0);
        assert_eq!(
            list(&plain)["extent"],
            json!({"cx": "1440000", "cy": "864000", "mm_w": 4000, "mm_h": 2400}),
            "两处尺寸里外面那一处：{}",
            list(&plain)
        );
        // 重写换了 EMU：4cm 在 LibreOffice 手里成了 1440180（换算 4001），两处一起跟着换
        assert_eq!(list(&rewritten)["extent"]["cx"], "1440180");
        assert_eq!(list(&rewritten)["extent"]["mm_w"], 4001);
        assert_eq!(list(&rewritten)["pic_extent"]["mm_w"], 4001);
        // 替代文字两处：外面那处两家都写，图里那处只有重写那份抄了
        assert_eq!(
            list(&plain)["alt_in_picture"],
            json!({"id": "0", "name": "dot.png", "descr": Value::Null, "descr_written": false}),
            "python-docx 在 pic:cNvPr 上写的是原文件名"
        );
        assert_eq!(list(&rewritten)["alt_in_picture"]["descr"], "一个红点");
        // 号是各家自己排的，而解出来的部件是同一个
        assert_eq!(list(&plain)["blip_id"], "rId9");
        assert_eq!(list(&rewritten)["blip_id"], "rId2");
        assert_eq!(list(&plain)["target"], "word/media/image1.png");
        assert_eq!(list(&rewritten)["target"], "word/media/image1.png");
        // 锁也是两处，一家只写外面那份
        assert_eq!(
            list(&plain)["pic_locks"],
            Value::Null,
            "python-docx 不写 a:picLocks"
        );
        assert_eq!(
            list(&rewritten)["pic_locks"],
            json!({"noChangeAspect": "1", "noChangeArrowheads": "1"})
        );
        // 浮在页上那一种：摆法在元素名上，绕排在元素名与属性上，位置一半在属性一半在字里
        assert_eq!(list(&plain)["placed"], "inline");
        assert_eq!(list(&plain)["wrap"], Value::Null);
        assert_eq!(list(&plain)["position_h"], Value::Null);
        assert_eq!(list(&floating)["placed"], "anchor");
        assert_eq!(list(&floating)["wrap"], "wrapSquare");
        assert_eq!(
            list(&floating)["wrap_written"],
            json!({"wrapText": "largest"}),
            "绕排的那一句开关写在属性上：{}",
            list(&floating)
        );
        assert_eq!(
            list(&floating)["position_h"],
            json!({"written": {"relativeFrom": "column"}, "element": "align", "value": "center"})
        );
        assert_eq!(
            list(&floating)["position_v"],
            json!({"written": {"relativeFrom": "paragraph"}, "element": "posOffset", "value": "635"})
        );
        // ODF 那一族：尺寸自带单位、摆法写在**属性**上、替代文字是**孩子元素**
        assert_eq!(
            list(&odt)["placed"],
            "as-char",
            "同一段稿子在 ODF 里写成 as-char：{}",
            list(&odt)
        );
        assert_eq!(
            list(&odt)["mm_w"],
            4001,
            "4.001cm 与 1440180 EMU 落到同一个数"
        );
        assert_eq!(list(&odt)["mm_h"], 2401);
        assert_eq!(list(&odt)["alt"], "一个红点");
        assert_eq!(list(&odt)["mime"], "image/png");
        assert_eq!(
            odt["structure"]["drawings"], 1,
            "这一族只数带 draw:image 的 frame"
        );
        // RTF 是第三种存法：尺寸写在三种单位上、格式有两份凭据、替代文字在形状属性那格里
        let rtf = run("images.rtf");
        let pic = rtf["structure"]["picture_list"][0].clone();
        assert_eq!(rtf["structure"]["pictures"], 1, "{rtf}");
        assert_eq!(
            pic["pixels"],
            json!({"w": "40", "h": "24"}),
            "像素那两个数不换算法 —— 这一族没写 DPI：{}",
            pic
        );
        assert_eq!(
            pic["goal"],
            json!({"w": "480", "h": "288", "unit": "twips", "mm_w": 847, "mm_h": 508}),
            "目标是 twips，换算与「那张纸」同一条整数式子"
        );
        assert_eq!(pic["scale"]["x"], "472", "缩放百分比也照写");
        assert_eq!(pic["blip"], "pngblip", "文件自己说这是什么格式");
        assert_eq!(pic["sig"], "png", "数据头几个字节自己也是凭据");
        assert_eq!(pic["sig_agrees"], json!(true), "两份凭据对得上");
        assert_eq!(pic["head_hex"], "89504e470d0a1a0a", "{pic}");
        assert_eq!(
            pic["alt"], "一个红点",
            "替代文字住在 wzDescription 那一格里"
        );
        assert_eq!(pic["props_written"], json!(true), "{pic}");
        assert_eq!(pic["truncated"], json!(false), "{pic}");
        assert_eq!(rtf["structure"]["pictures_with_alt_text"], 1, "{rtf}");
        // 同一句话在 docx 是 `wp:docPr/@descr`、在 odt 是 `svg:desc` 的字、在 rtf 是
        // `{\sn wzDescription}` 的值 —— 三处都读，字一字不差，而谁也不替谁圆场
        assert_eq!(
            [
                plain["structure"]["picture_list"][0]["alt"]["descr"].clone(),
                odt["structure"]["picture_list"][0]["alt"].clone(),
                pic["alt"].clone()
            ],
            [json!("一个红点"), json!("一个红点"), json!("一个红点")]
        );
        // 形状属性那格可以「写了而值是空的」：那与整个没有这一格是两件事，
        // 而「有替代文字」那本账数的是有字的，不是写了的
        let bare = run("notes.rtf");
        let bare_pic = bare["structure"]["picture_list"][0].clone();
        assert_eq!(bare_pic["props_written"], json!(true), "{bare_pic}");
        assert_eq!(
            bare_pic["alt_written"],
            json!(true),
            "这一格写了：{}",
            bare_pic
        );
        assert_eq!(bare_pic["alt"], "", "而里面是空的");
        assert_eq!(bare["structure"]["pictures"], 1, "{bare}");
        assert_eq!(bare["structure"]["pictures_with_alt_text"], 0, "{bare}");
    }

    /// 一串字里**有什么**与这一串字**说了什么**是两问：`w:t` 之外的孩子（制表、分页、
    /// 图、注的号、域指令）都按文件顺序整串交出来，而 `text` 只算字。注的号只有一个号，
    /// 跳进去要对**两种**注各开一本账 —— 脚注的 `2` 与尾注的 `2` 是两条不同的注。
    #[test]
    fn a_run_holds_more_than_words_and_note_ids_are_two_namespaces() {
        let toc = run("toc.docx");
        let ends = run("notes-end.docx");
        let odt = run("styled-text.odt");
        // 域指令不是页面上的字：这一串 text 交空串，那一串指令原样交给 instructions
        let instr = &toc["structure"]["run_formats"]["list"][2];
        assert_eq!(instr["text"], "", "指令串不写字：{instr}");
        assert_eq!(instr["contents"][0]["element"], "instrText");
        assert_eq!(instr["contents"][0]["written"]["space"], "preserve");
        assert_eq!(instr["instructions"][0], " TOC \\o \"1-2\" \\h");
        assert_eq!(
            toc["structure"]["run_formats"]["list"][1]["field_chars"],
            json!(["begin"])
        );
        assert_eq!(toc["structure"]["run_formats"]["field_runs"], 4);
        assert_eq!(toc["structure"]["run_formats"]["checked"], 21);
        assert_eq!(toc["structure"]["run_formats"]["runs_with_text"], 12);
        // 图与分页符也是这一串字的孩子，只是它们不写字
        assert_eq!(
            toc["structure"]["run_formats"]["list"][15]["contents"][0]["element"],
            "drawing"
        );
        assert_eq!(toc["structure"]["run_formats"]["list"][15]["text"], "");
        assert_eq!(
            toc["structure"]["run_formats"]["list"][17]["breaks"],
            json!(["page"])
        );
        // 批注的号也交，但它不跳注那两份部件（批注住在 comments.xml，另有一本账）
        assert_eq!(
            toc["structure"]["run_formats"]["list"][19]["refs"]["comment"],
            "0"
        );
        assert_eq!(
            toc["structure"]["run_formats"]["list"][19]["note"],
            Value::Null
        );
        assert_eq!(toc["structure"]["run_formats"]["runs_with_ref"], 1);
        assert_eq!(toc["structure"]["run_formats"]["ref_found"], 0, "{toc}");
        assert_eq!(toc["structure"]["run_formats"]["notes_in_parts"], 0);
        // 同一个号在两本账里：脚注的 2 排部件第 0 条，尾注的 2 排第 2 条
        let foot = &ends["structure"]["run_formats"]["list"][2];
        let end = &ends["structure"]["run_formats"]["list"][3];
        assert_eq!(foot["refs"]["footnote"], "2");
        assert_eq!(foot["note"]["kind"], "footnote");
        assert_eq!(foot["note"]["found"], json!(true));
        assert_eq!(foot["note"]["at"], 0, "{foot}");
        assert_eq!(end["refs"]["endnote"], "2");
        assert_eq!(end["note"]["kind"], "endnote");
        assert_eq!(end["note"]["at"], 2, "只按号对就会两条都落在第 0 条：{end}");
        assert_eq!(ends["structure"]["run_formats"]["list"][6]["note"]["at"], 1);
        assert_eq!(ends["structure"]["run_formats"]["notes_in_parts"], 3);
        assert_eq!(ends["structure"]["run_formats"]["notes_referenced"], 3);
        assert_eq!(ends["structure"]["run_formats"]["notes_unreferenced"], 0);
        assert_eq!(ends["structure"]["run_formats"]["ref_found"], 3, "{ends}");
        // ODF 那一条不包起来的字：四个问题都没有谁说话，四格各交 null 而不是缺键
        let piece = &odt["structure"]["run_formats"]["list"][0];
        assert_eq!(piece["element"], "#text");
        assert!(
            piece["switches"]
                .as_object()
                .is_some_and(|had| had.len() == 4),
            "四格都在而都空，缺键是「这一族没看」：{}",
            piece["switches"]
        );
        assert_eq!(piece["switches"]["bold"], Value::Null);
    }

    /// 「这句话是插进来的」「这句话是一条链接」都写在**壳**上（`w:ins` / `w:del` /
    /// `w:hyperlink`），而一串字自己一个字都没有：同一次插入一家一条壳、LibreOffice
    /// 重写成两条并换了一套号；删掉的字不住在 `w:t` 里，所以那一串的 `text` 是空串。
    #[test]
    fn a_shell_carries_the_author_the_time_and_the_link_id() {
        let rev = run("revisions.docx");
        let revlo = run("revisions-lo.docx");
        let toc = run("toc.docx");
        let notes = run("notes.docx");
        let r = rev["structure"]["run_formats"].clone();
        let lo = revlo["structure"]["run_formats"].clone();
        let t = toc["structure"]["run_formats"].clone();
        let n = notes["structure"]["run_formats"].clone();
        // 插入的壳上写着作者与时间，串里只有那些字
        let ins = &r["list"][3];
        assert_eq!(ins["wrapped"], "ins");
        assert_eq!(ins["wrapped_written"]["author"], "张三");
        assert_eq!(ins["wrapped_written"]["id"], "11");
        assert_eq!(ins["text"], "124000 元");
        // 同一次插入而重写之后是两条壳、号换了一套，字被拆成「数」与「单位」
        assert_eq!(lo["list"][3]["wrapped_written"]["id"], "0");
        assert_eq!(lo["list"][3]["text"], "124000 ");
        assert_eq!(lo["list"][4]["wrapped_written"]["id"], "1");
        assert_eq!(lo["list"][4]["text"], "元");
        assert_eq!(lo["list"][4]["wrapped_written"]["author"], "张三");
        assert_eq!(r["runs_wrapped"], 3);
        assert_eq!(r["wrapped_ins"], 2);
        assert_eq!(r["wrapped_del"], 1);
        assert_eq!(lo["runs_wrapped"], 5);
        assert_eq!(lo["wrapped_ins"], 3);
        assert_eq!(lo["wrapped_del"], 2, "{lo}");
        // 删掉的字不住在 w:t 里：text 是空串，而那些字照原样交在 delText 上
        let del = &r["list"][4];
        assert_eq!(del["wrapped"], "del");
        assert_eq!(del["text"], "", "删掉的字不在页面上：{del}");
        assert_eq!(del["contents"][0]["element"], "delText");
        assert_eq!(del["wrapped_written"]["author"], "李四");
        assert_eq!(del["wrapped_written"]["date"], "2026-03-06T11:45:00Z");
        assert_eq!(lo["list"][5]["contents"][0]["element"], "delText");
        // 链接的号是生产者自己排的：同一句话在两份件里分别是 rId2 与 rId9
        let link = &t["list"][14];
        assert_eq!(link["wrapped"], "hyperlink");
        assert_eq!(link["wrapped_written"]["id"], "rId2");
        assert_eq!(link["text"], "预算制度");
        assert_eq!(n["list"][8]["wrapped_written"]["id"], "rId9");
        // 没壳的那些串交 null，而不是空串
        assert_eq!(t["list"][13]["wrapped"], Value::Null);
        assert_eq!(t["list"][13]["wrapped_written"], Value::Null);
        assert_eq!(t["runs_wrapped"], 1);
        assert_eq!(t["wrapped_ins"], 0);
        assert_eq!(n["checked"], 13);
    }
}
