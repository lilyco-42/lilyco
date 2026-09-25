//! `lbin office-sheet` — 电子表格的结构：有几张表、哪张被藏起来、格子里装的是什么。
//!
//! 表格文件的常见问题跟文档不一样，它问的是**布局**：这张工作簿里有几张表（隐藏的也算，
//! 因为隐藏表往往是别人不让你看的那张）、每张自报的范围是多少、多少格子有值、
//! 其中多少是公式（公式的**缓存结果**在文件里可能存在也可能不存在，报出来时要说清读到的是哪一个）、
//! 有多少合并格、命名区域指向哪里、有没有外部工作簿与图表。
//!
//! OOXML 走 `xl/workbook.xml` + 每张表的 `xl/worksheets/sheetN.xml` + 共享字符串表；
//! 遗留 `.xls` 走 [`crate::biff`]（BIFF8 的记录表）。两条路的单元格都按引用（A1 形式）给出。

use serde_json::{json, Value};
use std::path::PathBuf;

use lilyco::prelude::*;

use crate::opack::{open, resolve_target, Family};
use crate::read::read_blob;
use crate::xmlscan;
use crate::zipread::{self, DEFAULT_MEMBER_CAP};

/// 报出工作簿的结构（T0 只读）
#[derive(App)]
#[app(
    name = "office-sheet",
    run = "run_office_sheet",
    about = "Report a spreadsheet's layout: every sheet with its workbook-order index, sheetId, relationship target, r:id and visibility (hidden and very-hidden sheets are listed, not skipped - they are usually the ones worth knowing about), each sheet's self-declared dimension, and per sheet the cell count, formula count, numeric/shared/inline-string split, merged ranges, hidden rows and columns. Also reports defined names (with what they point at), table parts (names, ranges, header rows), external-link workbook parts, chart and picture parts, styles/conditional formatting presence, and whether a calcChain exists. Shared strings are resolved so LABELSST cells carry their text; a formula cell reports the formula and says whether the file also cached a result (openpyxl-written files do not, and inventing a value there is exactly what this command refuses to do). Each cell also carries its number format: the style index on the cell is a row of xl/styles.xml cellXfs (not a format id), so a date is only a date once that hop is taken - the format code and, for date/time-formatted numeric cells, the ISO reading of the serial number are reported, honouring workbook.xml date1904 and reporting Excel's non-existent 1900-02-29 as written. A text cell like "12/23/2013" stays text. Legacy .xls goes through the BIFF8 record reader, and its hidden rows and columns come out of the two records the flags actually live in: bit 0x20 of the ROW record, and bit 0 of the COLINFO record (which states a range, expanded here - LibreOffice writes that record under the older id 0x007D while MS-XLS names 0x07D0 for BIFF8, so both ids are accepted). Which ROW bit means hidden was measured rather than recalled: three comparison files separate the two variables - row heights from 4pt to 250pt leave that bit alone, while hiding a single row sets exactly that bit. Hidden cells still count as cells. Spreadsheet comments are another hop: they are not in sheetN.xml at all - the sheet's own relationship part names the comments part, and the two producers measured here put it in two different places (openpyxl `xl/comments/comment1.xml` reached through an absolute target, LibreOffice `xl/comments1.xml` through `../comments1.xml`), with the author's name indexed through the `<authors>` list rather than written on the comment; ODF instead keeps the comment INSIDE the cell as `office:annotation`, which is exactly why the cell's own text skips that subtree. Authoring timestamps come back null on both producers because neither wrote one. Legacy .xls is a fourth spelling and stays inside the same stream: one record kind carries the text (offset 10 of its body is the character count it states for itself, and the first CONTINUE right after it opens with an encoding byte - 0 means one byte per character, 1 means two, which is the OPPOSITE of the BIFF8 fCompressed convention), while another record at the end of that sheet's own substream says which cell the note is on and who wrote it. The two lists are paired in order of appearance, every entry carries whole (were both self-stated counts satisfied), and both record counts are published per sheet so a mismatch shows up as data instead of a silently truncated list. This family writes no authoring timestamp, so date is null there; the reading was measured on LibreOffice-written .xls, and those two record numbers are not given spec names because MS-XLS assigns 0x001C to something else entirely. Whether a sheet can still be edited is reported per format, because the three spellings do not map onto one another: xlsx keeps two layers (workbookProtection plus each sheet's own sheetProtection, switches read in both the 1/0 and true/false spellings with an omitted one left omitted rather than false), .ods writes table:protected on the table itself together with the digest URI, and .xls has no workbook layer at all - PROTECT (0x0012), PASSWORD (0x0013) and SCENPROTECT (0x00DD) sit inside the locked sheet's own substream, so they are attributed per sheet and their raw values kept. ODF spreadsheets (.ods) are read on their own terms: cells carry the value-type the file wrote - written as-is, and null when no office:value-type was written, because guessing one from whether the cell has text is how the two readers here drifted apart (odp page tables write the attribute on none of their cells, seven of which hold text) - with office:value / date-value / boolean-value (no serial-number epoch to guess), positions are accumulated through table:number-columns-repeated runs (which routinely stand for 16000+ empty columns and are not counted), covered cells are tallied apart from content, merges come from the span attributes, a sheet's visibility is resolved through the automatic style it names, and hidden rows/columns are counted from table:visibility="collapse" on the element or in the row/column style it names (multiplying number-columns-repeated, so one element standing for three collapsed columns reports 3, not 1); each ODS cell additionally carries the number format it inherits - cell style, then style:data-style-name, then that number:*-style element (which lives in content.xml or styles.xml, and is reached through parent-style-name when the cell style itself names none) - reported as format_kind (taken from the element's own name, so a ¥ written as a literal text token stays a number-style), plus decimals, currency_symbol and a faithful format_tokens transcription; ODF has no format string, so none is invented. With --csv it also renders one sheet (by name, or by the 0-based index this command reports; --sheet picks it, default first) as RFC4180 CSV under { csv: {sheet, index, rows, columns, cells_skipped, line_end, text} } - date cells go out as the ISO reading of the serial number (legacy .xls takes the same hop too - the cell's ixfe indexes the XF records, whose format number names either a FORMAT record or a built-in id, and the epoch comes from DATEMODE; a file that never wrote DATEMODE gets the serial rather than a guessed 1900), a formula cell with no cached result goes out empty rather than guessed, holes are empty fields, and cells whose reference cannot be parsed as A1 are left out. Each xlsx sheet additionally carries its own print setup: the three elements `pageMargins`, `pageSetup` and `printOptions` are reported separately, and an element the file never wrote stays null rather than turning into false - openpyxl writes only the margins, while LibreOffice rewrites the same sheet with twelve `pageSetup` attributes (paperSize 9 and both dpi values included). Margins are handed over exactly as written, in the unit that family uses (`margin_unit`: inch, said once per sheet), rather than normalised to the 0.01mm integers office-doc reports - 0.5 versus 0.511811023622047 is the two producers' difference, and converting it away would erase the thing worth seeing. ODS and .xls print setup is deliberately not read: five LibreOffice-written .ods files name no page layout on the table element (the only link left is that producer's own `PageStyle_<sheet>` naming convention, not a spec hop), and while every .xls sheet does write a SETUP (0x00A1) record, nothing here can be a second reader for the field offsets inside it. A sheet's charts are two more hops: the sheet's own relationship part names a drawing part, and that drawing's relationships name the chart parts (openpyxl writes those targets absolute, LibreOffice relative - `resolve_target` eats both). So charts are attributed per sheet (`charts` and `chart_list`, and a sheet with none reports 0 rather than omitting the key): each entry reports its part, its title (a literal through `c:rich`, or a cell reference), whether the file cached any plotted values, and per plot group the kind (`barChart` / `lineChart`), every direct child's `val` as written (`barDir`, `grouping`, `gapWidth`...) with the axis ids kept separate because those are the producer's own numbering. The cached flag matters: openpyxl writes not one `c:pt`, so what the picture actually plots stays unknown and only the reference is handed over, while LibreOffice caches every point and self-states `ptCount` - that count and the number of points really found are both published, with whole saying whether they agree. Reference strings go out exactly as the file wrote them: the same cells read `'数据'!B1` in one producer's file and `数据!$B$1` in the other's, and the same text categories are a `c:numRef` in one and a `c:strRef` in the other - normalising either would be inventing what the file did not say. ODS keeps charts the ODF way - a `draw:frame` whose `draw:object` names an `Object N/` directory, whose own content.xml holds the picture - so a sheet reports its charts with the same shape as elsewhere plus a `local_table` transcription of the numbers the chart carried along. .xls still uses the BIFF object chain and is not read. Two more everyday things are read per sheet, because both answer questions people actually ask. Conditional formatting: each `conditionalFormatting` block keeps its `sqref` as written (one block may name two ranges), and each `cfRule` hands over every attribute the file wrote plus type/priority/operator named separately, its formula texts (entities decoded, so a written `$B2&gt;200` reads as `$B2>200`) and the shape inside `iconSet` / `colorScale` / `dataBar` - each `cfvo`'s type and val, each color's rgb exactly as written, since openpyxl writes 00FFFFFF where LibreOffice writes FFFFFFFF for the same white. `dxfId` is a hop: the rule names an index into `dxfs` in xl/styles.xml, so each rule says what it wrote, whether that index resolves, and which element paths that dxf contains (`font/b`, `font/color`...) - LibreOffice rewrites the same dxf with five entries where openpyxl wrote two, so nothing is folded into a shared bold-and-red shape. Priorities are each producer's own numbering (the same four rules read 1/2/3/4 in one file and 2/3/4/5 in the other), so they are reported rather than compared. Data validation: the `dataValidations` container's self-stated count is published next to how many were found and whether they agree, and each entry reports its range, type, operator, the formula1/formula2 texts as written and every attribute - including the two boolean spellings (allowBlank 1 versus true), the operator LibreOffice adds to a list rule, the formula2 it fills with 0, and the = it strips from a custom formula. ODS keeps these in number and table styles, .xls in BIFF records; neither is read, so the rules key is absent for those families rather than empty. Two everyday things per sheet come next, because both are questions people actually ask. The window: `view` hands over the attributes the `sheetView` element wrote (openpyxl writes four of them and spells booleans 1/0; LibreOffice rewrites the same sheet with fifteen and spells them true/false, so nothing is folded into one shared boolean), the one `pane` element - whose `state` keeps frozen and split apart because they are two different things - and every `selection` as written (three in one file with the first one not naming topLeft, four in the other, each with an activeCellId). Rewriting is not lossless: measured on a LibreOffice xlsx export, the pane with `state="split"` disappears entirely while the frozen one survives. The printed margins of the page: `header_footer` reports the six slot elements in a fixed order, each with whether the element is there at all, its text as written (entities decoded), the segments the `&L`/`&C`/`&R` markers name, and every other `&` sequence kept verbatim - `&&` is a literal ampersand and `&"Calibri"` is one whole code, so the scan walks codes rather than counting characters. Absent and empty stay apart: a sheet openpyxl never wrote reports present false with null text, while the same sheet after LibreOffice writes the element with empty contents (present true, text ""). ODS keeps both in its page styles, the same hop print setup could not take, and .xls keeps them in WINDOW1/WINDOW2 and HEADER/FOOTER records no second reader here can check, so both keys are absent for those families. Three more ledgers per sheet answer the three everyday "why" questions - why a column shows too little, why a row will not take what I typed, and whether this range really is one table. `layout` reports the attributes `sheetFormatPr` wrote (the two producers do not even name the same thing for a default width: `baseColWidth` in one file, `defaultColWidth` in the other), every `col` as written (the same column reads 22.5 in one file and 20.47 in the other, hidden is spelled 1 in one and true in the other, and nothing is converted), how many `col` elements there are next to how many columns they cover (summed from each element's own min and max, never expanded, and null rather than 0 when there is none), and three row counts - rows in the sheet, rows that wrote an `ht`, rows that said anything at all about height or visibility - plus those rows as written. `filter` says whether there is an `autoFilter`, its range as written, every `filterColumn` with its switches and the values being filtered out, and the `filterMode` the sheet may or may not state on `sheetPr` (one producer writes it for every sheet, true and false, the other never writes it). Table objects are another hop: `tableParts` names relationship ids only, and the parts come out of the sheet's own relationship table, the same linking style charts use; each part reports its attributes as written (name and displayName, ref, headerRowCount, plus the two totals switches LibreOffice adds), its self-stated `tableColumns/@count` next to how many were found with `whole`, the column names themselves as written (openpyxl takes the first row of the range as names, so the second column is literally called 10, and both producers carry that name unchanged), and its own `autoFilter` - which can cover a different range than the sheet's filter (A1:B3 next to A1:C3 on one sheet), so both are reported instead of picking one. ODS reads its sizes too, and they are a third shape - which is the whole reason this ledger exists: a table-column or table-row element writes only how many columns or rows it stands for (number-columns-repeated - LibreOffice pads every sheet out to exactly 16384 columns that way, measured on six files) and occasionally its own visibility, while the width and height sit one hop away in the automatic style the element names. So layout for .ods reports each axis as elements / spans (how many elements there are, next to how many columns they cover - and neither is the sheet's columns key, which counts only the rightmost cell that has content), with resolved / with_size / optimal / spoken_visibility tallied over every element, then per element: the style it named, whether that style was found in this part, the length as written plus its 0.01mm reading (unit is said once per sheet), the style's own use-optimal flag, the style's visibility kept apart from the element's because they are two ways to say the same thing, and the style's parent-style-name - the parent chain is deliberately NOT followed (no file measured here writes one), it is only reported so a missing width can explain itself. The four numbers a table may state about itself (number-columns, number-rows, default-column-width, default-row-height) come back as four keys, null when unwritten: LibreOffice writes none of them, and absent is not zero. filter and tableParts are OOXML-only and stay absent for ODS; .xls has none of the three. A result that is not a number goes out as the file wrote it: an error cell's <v> already IS the display string (#DIV/0!), so nothing is prefixed - both readers used to prepend a "#错误 " tag that appears in no file, and no producer here recomputed one until errors-lo.xlsx, so the branch stayed untested on both sides; a text result (t="str") is likewise handed over verbatim, and error_cells / string_result_cells count the two per sheet. kind_written says whether that cell wrote a t at all: the spec default is numeric, but openpyxl omits the attribute on every formula cell while LibreOffice writes t="n" for all of them, so kind alone would read one producer's silence as the other's statement - cells_with_written_type counts those who spoke. ODF spells the same cell a third way: office:value-type="string" with an EMPTY office:string-value, the displayed text only in <text:p>, plus a calcext:value-type="error" that is not followed - so an error reads kind string there, and because that export localises the name, one broken VLOOKUP is #VALUE! in its xlsx and 错误:502 in its ods: each goes out as written and they are never reconciled. A string cell keeps the whitespace the file wrote - no trimming, because the two spaces in a <t xml:space="preserve"> are the file's own and LibreOffice's own CSV export carries them too - and it additionally reports runs: how the file split that string (a rich one is several <r> elements, each with its own rPr attributes as written, while a plain <t> has none at all), one entry per run with element, text, space, props_written, props_attrs and format (the rPr's own attributes and its child elements - the two producers here put the shape on children like <b val="true"/> and write no attribute on rPr at all), next to run_total, rich_string and space_preserved on the cell and cells_with_runs / cells_with_preserved_space per sheet. The string table is also held against what it states about itself: sst/@count (references) and sst/@uniqueCount (entries) go out as written beside how many si were really found, because one string used by two cells is a file that says count 8 with seven entries and both numbers are right. ODF spells the same cell text a fourth way: spaces and tabs are MARKERS, not characters - text:s (with text:c saying how many spaces that one stands for), text:tab and text:line-break - so those are expanded here (LibreOffice's own CSV export of the same .ods keeps every one of them), and each cell says how many text:span and how many marker elements the file really wrote (spans / specials). What a cell LOOKS like is another hop: the `s=` index names one `cellXfs` row, and that row writes only three numbers - `fontId` / `fillId` / `borderId` - whose text lives in the `fonts` / `fills` / `borders` tables. Each cell therefore carries `style_attrs` (what that xf row wrote, verbatim), the three resolved rows (`style_font` / `style_fill` / `style_border`, each as written attributes plus child elements, and those children's own children one level deeper - the fill colour is not on `patternFill` but on its `fgColor` / `bgColor` child, so a one-level row can say a cell is solid without being able to say which colour), `style_alignment`, the three ids, `style_found`, and three tri-state switches (`style_bold` / `style_filled` / `style_wrapped`, null when the file said nothing) counted per sheet as cells_bold / cells_filled / cells_wrapped. style_written says whether the cell wrote an `s` at all - one producer writes s="0" on every cell, the other leaves the default unwritten. The tables report their own `count` next to how many rows were found (fonts / fills / borders / cellXfs / cellStyleXfs under `workbook.styles`), and nothing is normalised between producers: openpyxl's placeholder fill is an EMPTY `<patternFill/>` while LibreOffice writes `patternType="none"`, the same bold switch is spelled `val="1"` in one file and `val="true"` in the other, an `indexed="64"` colour comes back as `rgb="FF000000"` after the rewrite, and LibreOffice turns a `lightGrid` pattern into a `solid` one with different colours - a rewrite is not lossless, so all of it is handed over as written. .xls keeps its formats in BIFF XF/FORMAT records (already reported through the number-format hop) and ODS keeps cell looks in named styles, so neither carries these keys. ODS page styles get their own top-level ledger, because nothing inside a .ods names the page style a sheet prints with: every style:master-page is reported with its six slots, a slot the file wrote while saying display=false stays distinct from a slot the file never wrote, and paragraphs living in the style:region-left / style:region-right halves are counted too - LibreOffice puts the sheet-name and title fields in the left half and the date and time in the right, so a reader that walks only direct children reports a written header as an empty one. What a sheet prints is a separate ledger (print_ranges) because neither family keeps it on the sheet the way office software's own UI suggests: OOXML writes two reserved defined names (_xlnm.Print_Area and _xlnm.Print_Titles) in the workbook part, attributed through localSheetId which counts the ORDER of the sheets - not sheetId and not r:id, both of which are numbered separately by each producer; one name can carry several comma-separated ranges, so the whole sentence and the split pieces are both reported, and LibreOffice's rewrite of this fixture drops the quotes around every sheet name while changing nothing else (quoted_entries counts that on its own). ODF keeps it in two places: an attribute on the table itself (space-separated, with a third spelling of cell addresses) plus a set of table:named-range and table:named-expression entries kept for the Excel round trip - and the repeated-row choice exists ONLY in that second place, so reading the table attribute alone loses it; all five entries in the fixture share one base-cell-address, which means attribution lives in the address string, not in that pointer. Returns { path, format, sheets, page_styles, print_ranges, protection, csv, workbook, defined_names, tables, external_links, parts, notes }. The formula element itself is a separate ledger (formula_elems): Excel stores one column of look-alike formulas as a shared group, where the master cell writes <f t=shared ref=B1:B8 si=0>A1*2</f> and the seven followers write <f t=shared si=0/> with NO text at all - so how many cells carry a formula (16) and how many carry text (9) are two numbers, and merging them would read the file not writing that formula as the cell having no formula. attrs_seen is reported in the order the file wrote them (t, ref, si), not alphabetically. Three producers, three styles: openpyxl writes no attribute on f at all, LibreOffice's rewrite drops the shared group (16 full texts) while writing aca=false on every one of them, and the same sheet as ods moves the formula onto the cell as table:formula with all 16 texts translated row by row (of:=[.A2]*2) - that family has no si/ref/shared slot at all, so those keys are simply absent there (not 0). One more pair kept apart: whether a v element is present (cached_written) and what it holds (cached, as written - openpyxl's 16 are empty tags), so a tag is never reported as a value. This ledger does not expand shared groups or replay formula semantics, and legacy .xls pages do not carry the key at all. "
)]
pub struct OfficeSheet {
    /// 表格文件（xlsx / xlsm / xls / ods）
    #[arg(about = "Spreadsheet to inspect", must_exist = true)]
    path: PathBuf,

    /// 每张表最多列多少个格子（总数照实给）
    #[arg(
        about = "List at most this many cells per sheet",
        default = 200,
        min = 1
    )]
    limit: u64,

    /// 顺带交一份 CSV（RFC4180：带逗号/引号/换行的字段加引号，日期给 ISO）
    #[arg(about = "Also render one sheet as CSV")]
    csv: bool,

    /// `--csv` 要哪张表：表名，或 `index` 那个从 0 起的序号；不给就是第一张
    #[arg(about = "Sheet for --csv: name or 0-based index", default = "")]
    sheet: String,

    /// 最多读多少字节
    #[arg(about = "Read at most this many bytes", default = 67108864)]
    max_bytes: u64,
}

/// CLI 的 `#[arg(default = N)]` 与各端省略参数时的回退值必须是同一个数
const LIMIT_DEFAULT: usize = 200;

fn run_office_sheet(app: &OfficeSheet, ctx: &Context) -> Result<Value, AppError> {
    let start = std::time::Instant::now();
    let blob = read_blob(&app.path, app.max_bytes).map_err(AppError::InvalidInput)?;
    ctx.emit(Progress::Started {
        total: Some(blob.size),
        message: Some("reading the workbook layout".to_string()),
    });
    let doc = open(&blob.bytes);
    let bytes = &blob.bytes[..];
    let limit = crate::opack::take_limit(app.limit, LIMIT_DEFAULT);
    if doc.family == Family::Ooxml && doc.app == "excel" {
        let workbook = xml(bytes, "xl/workbook.xml")
            .ok_or_else(|| AppError::InvalidInput("包里读不到 xl/workbook.xml".to_string()))?;
        let root = xmlscan::parse_str(&workbook.as_text());
        // 工作簿的顺序是真相：r:id → 部件路径，靠 workbook.xml.rels 对上
        let mut by_id: Vec<(String, String)> = Vec::new();
        if let Some(rels) = xml(bytes, "xl/_rels/workbook.xml.rels") {
            let rel_root = xmlscan::parse_str(&rels.as_text());
            for one in rel_root.descendants("Relationship") {
                let id = one.attr("Id").unwrap_or_default().to_string();
                let target = one.attr("Target").unwrap_or_default();
                by_id.push((id, resolve_target("xl", target)));
            }
        }
        let shared = shared_strings(bytes, limit);
        // 格子写的 `s="3"` 是 `cellXfs` 的**下标**，不是格式号：不绕这一层，
        // 一个日期永远只是「一个数」（41631）。
        let styles = crate::numfmt::read_styles(bytes);
        let mut notes = doc.notes.clone();
        notes.extend(styles.notes.iter().cloned());
        let mut sheets: Vec<Value> = Vec::new();
        let mut grid_names: Vec<String> = Vec::new();
        let mut grids: Vec<Vec<(usize, usize, String)>> = Vec::new();
        let mut grid_skipped = 0usize;
        // 条件格式的规则只写一个 dxfId，真样式在 styles.xml 的 dxfs 那一跳上
        let (dxf_written, dxfs) = dxf_table(bytes);
        let dxf_whole = match &dxf_written {
            None => true,
            Some(raw) => raw.trim().parse::<usize>().ok() == Some(dxfs.len()),
        };
        let mut totals = json!({
            "cells": 0, "formulas": 0, "numeric": 0, "shared_strings": 0,
            "inline_strings": 0, "merged": 0, "hidden_rows": 0, "hidden_cols": 0,
            "dates": 0, "comments": 0, "charts": 0, "conditional_rules": 0, "validations": 0,
            // bump 只往已有的键上加，所以这几本新账要在totals里先占个位（0 是「数过了没有」）
            "error_cells": 0, "string_result_cells": 0, "cells_with_written_type": 0,
            "cells_with_runs": 0, "cells_with_preserved_space": 0,
            "cells_bold": 0, "cells_filled": 0, "cells_wrapped": 0,
            "cells_without_style_written": 0,
        });
        for (index, one) in root.descendants("sheet").iter().enumerate() {
            let name = one.attr("name").unwrap_or_default().to_string();
            let rid = one.attr_local("id").unwrap_or_default().to_string();
            let part = by_id
                .iter()
                .find(|(one_id, _)| *one_id == rid)
                .map(|(_, path)| path.clone())
                .unwrap_or_else(|| format!("xl/worksheets/sheet{}.xml", index + 1));
            let state = match one.attr("state").unwrap_or("visible") {
                "hidden" => "hidden",
                "veryHidden" => "very-hidden",
                _ => "visible",
            };
            // 批注那两跳：表 → 它自己的关系表 → 批注部件
            let comments = sheet_comments(bytes, &part, limit);
            let counted = comments.len();
            let mut entry = json!({
                "index": index,
                "name": name,
                "sheet_id": one.attr("sheetId"),
                "state": state,
                "r_id": rid,
                "part": part,
                "comments": counted,
                "comment_list": comments,
            });
            bump(&mut totals, "comments", counted);
            match xml(bytes, &part) {
                Some(member) => {
                    let sheet_root = xmlscan::parse_str(&member.as_text());
                    let dimension = sheet_root
                        .descendants("dimension")
                        .first()
                        .and_then(|one| one.attr("ref"))
                        .unwrap_or_default()
                        .to_string();
                    // 打印那份设置是这张表自己的（三个元素各自在不在）
                    entry["print_setup"] = xlsx_print_setup(&sheet_root);
                    // 窗口状态与页眉页脚同理：这张表自己写了什么才算，没写的交 null
                    entry["view"] = xlsx_view(&sheet_root, limit);
                    entry["header_footer"] = xlsx_header_footer(&sheet_root);
                    // 尺寸与筛选也是这张表自己写的；表对象要顺关系表跳一跳
                    entry["layout"] = xlsx_layout(&sheet_root, limit);
                    entry["filter"] = xlsx_filter(&sheet_root, limit);
                    let sized = sheet_tables(bytes, &part, &sheet_root, limit);
                    entry["tables"] = sized["tables"].clone();
                    entry["table_list"] = sized["table_list"].clone();
                    entry["table_parts"] = sized["parts"].clone();
                    // 图要再跳两跳才到：表 → 自己的关系表 → 画法部件 → 它的关系表 → 图
                    let charts = sheet_charts(bytes, &part, limit);
                    let charted = charts.len();
                    let uncached = charts
                        .iter()
                        .filter(|one| one["cached"].as_bool() != Some(true))
                        .count();
                    let rules = sheet_rules(&sheet_root, &dxfs, limit);
                    let conditioned = rules["conditional"]
                        .as_array()
                        .map(|one| {
                            one.iter()
                                .map(|had| had["rules"].as_u64().unwrap_or(0) as usize)
                                .sum::<usize>()
                        })
                        .unwrap_or_default();
                    let validated = rules["validations"]["list"]
                        .as_array()
                        .map(|one| one.len())
                        .unwrap_or_default();
                    bump(&mut totals, "conditional_rules", conditioned);
                    bump(&mut totals, "validations", validated);
                    entry["rules"] = rules;
                    entry["charts"] = json!(charted);
                    entry["chart_list"] = Value::Array(charts);
                    bump(&mut totals, "charts", charted);
                    if uncached > 0 {
                        notes.push(format!(
                            "这张表上 {} 张图的数值没有缓存（图里画的是哪些数判不住，只能交引用）",
                            uncached
                        ));
                    }
                    let mut cells: Vec<Value> = Vec::new();
                    let mut count = 0usize;
                    let mut formulas = 0usize;
                    let mut numeric = 0usize;
                    let mut shared_count = 0usize;
                    let mut inline = 0usize;
                    let mut cached = 0usize;
                    let mut dates = 0usize;
                    let mut errored = 0usize;
                    let mut str_result = 0usize;
                    let mut typed = 0usize;
                    let mut rich = 0usize;
                    let mut preserved = 0usize;
                    let mut bolded = 0usize;
                    let mut colored = 0usize;
                    let mut wrapped = 0usize;
                    let mut no_style = 0usize;
                    let mut grid: Vec<(usize, usize, String)> = Vec::new();
                    for cell in sheet_root.descendants("c") {
                        count += 1;
                        let reference = cell.attr("r").unwrap_or_default().to_string();
                        // `t` 没写时按规范默认 n，但「默认出来的 n」与「文件写了 n」是两件事：
                        // openpyxl 给公式格一个都不写，LibreOffice 重写同一份时连数字格也写
                        let kind_written = cell.attr("t").is_some();
                        // 同一个道理用在样式号上：`s` 没写按 0 查，但「按默认查第 0 条」与
                        // 「文件自己写了 s="0"」是两件事（LibreOffice 每格都写，openpyxl 不写）
                        let style_written = cell.attr("s").is_some();
                        let kind = cell.attr("t").unwrap_or("n").to_string();
                        let style_index = cell
                            .attr("s")
                            .and_then(|raw| raw.trim().parse::<usize>().ok())
                            .unwrap_or(0);
                        let value = cell.child("v").map(|one| one.text().trim().to_string());
                        let formatted = merge(
                            styles.cell_format(style_index, value.as_deref(), kind.as_str()),
                            styles.appearance(style_index),
                        );
                        // 长相那三本合计只数「文件说了 true」的：null（没写、指不到那张表）
                        // 与 false（写了关）都不算，两条各在逐格的账里看得见
                        if formatted["style_bold"].as_bool() == Some(true) {
                            bolded += 1;
                        }
                        if formatted["style_filled"].as_bool() == Some(true) {
                            colored += 1;
                        }
                        if formatted["style_wrapped"].as_bool() == Some(true) {
                            wrapped += 1;
                        }
                        if cell.attr("s").is_none() {
                            no_style += 1;
                        }
                        if matches!(
                            formatted["format_kind"].as_str().unwrap_or(""),
                            "date" | "datetime" | "time"
                        ) {
                            dates += 1;
                        }
                        let formula = cell.child("f").map(|one| one.text().trim().to_string());
                        if let Some(raw) = &formula {
                            formulas += 1;
                            let _ = raw;
                        }
                        if cell.child("is").is_some() {
                            inline += 1;
                        }
                        if kind == "s" {
                            shared_count += 1;
                        } else if let Some(raw) = &value {
                            if raw.parse::<f64>().is_ok() {
                                numeric += 1;
                            }
                        }
                        if kind_written {
                            typed += 1;
                        }
                        if kind == "e" {
                            errored += 1;
                        } else if kind == "str" {
                            str_result += 1;
                        }
                        // 这条格子的字如果是一个「串」（共享的或 inline 的），它的整串与
                        // 分段一起拿：分段就是富文本，而整串按文件写的原样（不 trim）
                        let parts = match kind.as_str() {
                            "s" => value
                                .as_ref()
                                .and_then(|raw| raw.parse::<usize>().ok())
                                .and_then(|which| shared.entries.get(which).cloned()),
                            "inlineStr" => cell.child("is").map(|one| string_parts(one, limit)),
                            _ => None,
                        };
                        if let Some(one) = &parts {
                            if one.rich {
                                rich += 1;
                            }
                            if one.preserved {
                                preserved += 1;
                            }
                        }
                        let text = match kind.as_str() {
                            "s" => parts.as_ref().map(|one| one.text.clone()).or_else(|| {
                                value
                                    .as_ref()
                                    .and_then(|raw| raw.parse::<usize>().ok())
                                    .map(|which| format!("#SST 索引 {which} 越界"))
                            }),
                            "inlineStr" => parts.as_ref().map(|one| one.text.clone()),
                            "str" => value.clone(),
                            // 错误格的文件自己就写着显示的那串（`<v>#DIV/0!</v>`），
                            // 前面再加一句「#错误」是两份读者一起替文件编的话 ——
                            // LibreOffice 自己的 CSV 导出交的就是 `#DIV/0!`，一字不加
                            "e" => value.clone(),
                            "b" => value.clone().map(|one| {
                                if one == "1" {
                                    "TRUE".to_string()
                                } else {
                                    "FALSE".to_string()
                                }
                            }),
                            _ => match value.as_ref().and_then(|raw| raw.parse::<f64>().ok()) {
                                Some(one) => Some(format!("{one}")),
                                None => value.clone(),
                            },
                        };
                        // `<v></v>` 是「有标签、没值」：openpyxl 就是这么写的，
                        // 把空串当成缓存结果等于替文件编一个数
                        if formula.is_some()
                            && value.as_ref().is_some_and(|raw| !raw.trim().is_empty())
                        {
                            cached += 1;
                        }
                        if grid.len() < MAX_GRID_CELLS {
                            if let Some((row, col)) = split_ref(&reference) {
                                let display = match formatted["as_date"].as_str() {
                                    Some(one) => one.to_string(),
                                    None => text.clone().unwrap_or_default(),
                                };
                                grid.push((row, col, display));
                            }
                        } else {
                            grid_skipped += 1;
                        }
                        if cells.len() < limit {
                            cells.push(merge(
                                json!({
                                    "ref": reference,
                                    "kind": kind,
                                    "kind_written": kind_written,
                                    "style_written": style_written,
                                    "value": numeric_or_text(text),
                                    "formula": formula,
                                    // 分段本身：一家把整格写成一段，另一家按字体 fallback 切成两段
                                    "runs": parts
                                        .as_ref()
                                        .map(|one| Value::Array(one.runs.clone()))
                                        .unwrap_or(Value::Null),
                                    "run_total": parts
                                        .as_ref()
                                        .map(|one| json!(one.run_total))
                                        .unwrap_or(Value::Null),
                                    "rich_string": parts
                                        .as_ref()
                                        .map(|one| json!(one.rich))
                                        .unwrap_or(Value::Null),
                                    "space_preserved": parts
                                        .as_ref()
                                        .map(|one| json!(one.preserved))
                                        .unwrap_or(Value::Null),
                                }),
                                formatted,
                            ));
                        }
                    }
                    entry["dimension"] = json!(dimension);
                    entry["cells"] = json!(count);
                    let hidden_rows = sheet_root
                        .descendants("row")
                        .iter()
                        .filter(|one| hidden_on(one))
                        .count();
                    let hidden_cols = sheet_root
                        .descendants("col")
                        .iter()
                        .filter(|one| hidden_on(one))
                        .map(|one| col_span(one))
                        .sum::<usize>();
                    entry["hidden_rows"] = json!(hidden_rows);
                    entry["hidden_cols"] = json!(hidden_cols);
                    entry["listed"] = json!(cells.len());
                    entry["formulas"] = json!(formulas);
                    entry["numeric"] = json!(numeric);
                    entry["shared_string_cells"] = json!(shared_count);
                    entry["inline_strings"] = json!(inline);
                    entry["date_cells"] = json!(dates);
                    entry["formula_cells_with_cached_value"] = json!(cached);
                    entry["error_cells"] = json!(errored);
                    entry["string_result_cells"] = json!(str_result);
                    entry["cells_with_written_type"] = json!(typed);
                    entry["cells_with_runs"] = json!(rich);
                    entry["cells_with_preserved_space"] = json!(preserved);
                    entry["cells_bold"] = json!(bolded);
                    entry["cells_filled"] = json!(colored);
                    entry["cells_wrapped"] = json!(wrapped);
                    entry["cells_without_style_written"] = json!(no_style);
                    bump(&mut totals, "cells_with_runs", rich);
                    bump(&mut totals, "cells_with_preserved_space", preserved);
                    bump(&mut totals, "cells_bold", bolded);
                    bump(&mut totals, "cells_filled", colored);
                    bump(&mut totals, "cells_wrapped", wrapped);
                    bump(&mut totals, "cells_without_style_written", no_style);
                    entry["merged"] = json!(sheet_root.descendants("mergeCell").len());
                    entry["rows"] = json!(sheet_root.descendants("row").len());
                    entry["cell_list"] = json!(cells);
                    grid_names.push(name.clone());
                    grids.push(grid);
                    bump(&mut totals, "cells", count);
                    bump(&mut totals, "formulas", formulas);
                    bump(&mut totals, "numeric", numeric);
                    bump(&mut totals, "shared_strings", shared_count);
                    bump(&mut totals, "inline_strings", inline);
                    bump(&mut totals, "dates", dates);
                    bump(&mut totals, "error_cells", errored);
                    bump(&mut totals, "string_result_cells", str_result);
                    bump(&mut totals, "cells_with_written_type", typed);
                    bump(&mut totals, "hidden_rows", hidden_rows);
                    bump(&mut totals, "hidden_cols", hidden_cols);
                    bump(
                        &mut totals,
                        "merged",
                        sheet_root.descendants("mergeCell").len(),
                    );
                }
                None => {
                    entry["error"] = json!("表的工作流部件读不出来");
                }
            }
            sheets.push(entry);
        }
        let defined: Vec<Value> = root
            .descendants("definedName")
            .iter()
            .take(limit)
            .map(|one| json!({"name": one.attr("name").unwrap_or_default(), "text": one.text()}))
            .collect();
        let external: Vec<String> = doc
            .entries
            .iter()
            .map(|one| one.name.clone())
            .filter(|one| one.starts_with("xl/externalLinks/externalLink"))
            .collect();
        // 保护这份账：工作簿一层、每张表一层，两层的开关还各说各的话
        // （openpyxl 写 `sheet="1"`，LibreOffice 重写同一份东西写 `sheet="true"`）
        let mut locks: Vec<Value> = Vec::new();
        for one in &sheets {
            let name = one["name"].as_str().unwrap_or_default().to_string();
            let part = one["part"].as_str().unwrap_or_default().to_string();
            match xml(bytes, &part) {
                Some(member) => {
                    let sheet_root = xmlscan::parse_str(&member.as_text());
                    locks.push(crate::protect::xlsx_sheet(&sheet_root, &name));
                }
                None => locks.push(json!({"name": name, "element": false, "protected": false})),
            }
        }
        // 共享字符串表那份自报的账：`count` 是「引用了几次」，`uniqueCount` 是「几条不重复」，
        // 两个数都不替它们圆 —— 一条 `甲` 被两个格子引用时，七个 si 配八个 count 是对的
        let sst_matches = shared
            .unique_written
            .as_ref()
            .and_then(|raw| raw.trim().parse::<usize>().ok())
            == Some(shared.entries.len());
        let sst_rich = shared.entries.iter().filter(|one| one.rich).count();
        let sst_preserved = shared.entries.iter().filter(|one| one.preserved).count();
        let result = json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "spreadsheetml",
            "workbook": {
                "sheets": sheets.len(),
                "hidden_sheets": sheets.iter().filter(|one| one["state"] != "visible").count(),
                "date1904": json!(styles.year1904),
                "shared_strings": shared.entries.len(),
                "sst_count_written": shared.count_written,
                "sst_unique_written": shared.unique_written,
                "sst_unique_matches": sst_matches,
                "sst_with_runs": sst_rich,
                "sst_with_preserved_space": sst_preserved,
                "views": root.descendants("workbookView").len(),
                "calculation_mode": root.descendants("calcPr").first().and_then(|one| one.attr("fullCalcOnLoad")).map(|one| one.to_string()),
                "has_calc_chain": xml(bytes, "xl/calcChain.xml").is_some(),
                "styles": styles.ledger.clone(),
                "dxfs": {"written": dxf_written, "found": dxfs.len(), "whole": dxf_whole},
                "styles_part": xml(bytes, "xl/styles.xml").is_some(),
                "tables": doc.entries.iter().filter(|one| one.name.starts_with("xl/tables/")).count(),
                "charts": doc.entries.iter().filter(|one| one.name.starts_with("xl/charts/")).count(),
                "media": doc.entries.iter().filter(|one| one.name.starts_with("xl/media/")).count(),
                "totals": totals,
            },
            "sheets": sheets,
            "protection": json!({
                "workbook": crate::protect::xlsx_workbook(&root),
                "sheets": locks,
            }),
            "csv": csv_report(app.csv, &grid_names, &grids, &app.sheet, grid_skipped),
            "defined_names": defined,
            // 打印区域与重复标题行：这一族把它们写成两条保留名，归属是 localSheetId 那个序号
            "print_ranges": crate::print_ranges::xlsx(&root, limit),
            // 公式那枚 <f> 自己写了什么：共享组的跟随格在文件里没有正文
            "formula_elems": crate::formula_elems::xlsx(bytes, limit),
            "external_links": external,
            "notes": notes,
        });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    if doc.family == Family::Odf && doc.app == "excel" {
        let book = crate::odsheet::read(bytes);
        // 图那份账要另走 content.xml：ODS 的 draw:frame 就住在它属于的那张 table:table 里面
        let content_root = xml(bytes, "content.xml").map(|one| xmlscan::parse_str(&one.as_text()));
        let hosts: Vec<(&str, &xmlscan::Node)> = content_root
            .iter()
            .flat_map(|root| {
                root.descendants("table")
                    .into_iter()
                    .map(|one| (crate::odfchart::attr_of(one, "name").unwrap_or(""), one))
                    .collect::<Vec<(&str, &xmlscan::Node)>>()
            })
            .collect();
        let mut notes = book.notes.clone();
        // 格式在另一跳上：格子 → 单元格样式 → `style:data-style-name` → `number:*-style`，
        // 而那棵元素树可能坐在 content.xml，也可能坐在 styles.xml
        let styles = crate::odstyle::Styles::read(bytes);
        notes.extend(styles.notes.iter().cloned());
        notes.push(
            "ODF 的数字格式是一棵元素树，不是 Excel 那种格式串：这里逐条抄成 \
             `format_tokens`（`year`、`text:-`…），不替它重构 `yyyy-mm-dd`；\
             类别只看样式元素自己的名字 —— 屏上带 ¥ 字面量的那份其实是个 number-style"
                .to_string(),
        );
        notes.push(
            "ODF 的格子里不写序列数：日期就是 `office:date-value` 那个 ISO 串，\
             所以这边没有 1900 / 1904 那套基准要猜；显示文本（如 `12.5%`）与值是两样东西"
                .to_string(),
        );
        notes.push(
            "ODF 的列宽与行高**不在列/行元素上**：元素只写 `number-columns-repeated` \
             （实测六份 LibreOffice 转出来的 .ods 都把每张表补到整 16384 列）与偶尔一句 \
             `table:visibility`，尺寸在它点名的那份自动样式里 —— 所以逐条都交 `style`（点了谁）、\
             `resolved`（那份样式在这件里找着没有）与 `size`（文件自己写的那一串），\
             `size_mm` 才是换成 0.01mm 的整数。样式若写着 `style:parent-style-name` 就把它报出来，\
             但**不顺父链再跳**：实测的文件里一个都没写"
                .to_string(),
        );
        let grid_names: Vec<String> = book.sheets.iter().map(|one| one.name.clone()).collect();
        let grids: Vec<Vec<(usize, usize, String)>> = book
            .sheets
            .iter()
            .map(|one| {
                one.cells
                    .iter()
                    .filter_map(|had| {
                        split_ref(&had.reference).map(|(row, col)| (row, col, ods_display(had)))
                    })
                    .collect()
            })
            .collect();
        let mut sheets: Vec<Value> = Vec::new();
        let mut totals = json!({
            "cells": 0, "formulas": 0, "dates": 0, "merged": 0, "covered": 0,
            "hidden_rows": 0, "hidden_cols": 0, "comments": 0, "charts": 0,
        });
        for (index, one) in book.sheets.iter().enumerate() {
            // 这一张表上的图：先按表名找到它自己的 `table:table`，再走 frame → object 那一跳
            let charts = hosts
                .iter()
                .find(|(name, _)| *name == one.name.as_str())
                .map(|(_, node)| crate::odfchart::charts_in(bytes, node))
                .unwrap_or_default();
            let charted = charts.len();
            let mut types = serde_json::Map::new();
            for had in &one.cells {
                // 只数写了类型的格子：旁边的 `cells` 是全部，差额就是「文件没说类型」
                if let Some(key) = &had.value_type {
                    let next = types.get(key).and_then(|one| one.as_u64()).unwrap_or(0) + 1;
                    types.insert(key.clone(), json!(next));
                }
            }
            sheets.push(json!({
                "index": index,
                "name": one.name,
                "state": if one.visible { "visible" } else { "hidden" },
                "rows": one.rows,
                "columns": one.columns,
                "cells": one.cells.len(),
                "formulas": one.formulas(),
                "date_cells": one.date_cells(),
                "merged": one.merged,
                "covered": one.covered,
                "hidden_rows": one.hidden_rows,
                "hidden_cols": one.hidden_cols,
                "comments": one.comments.len(),
                "comment_list": one.comments.iter().cloned().take(limit).collect::<Vec<Value>>(),
                "value_types": Value::Object(types),
                "layout": ods_layout(one, limit),
                "cell_list": one.cells.iter().take(limit).map(|had| merge(had.to_json(), styles.for_cell(had.style_name.as_deref()))).collect::<Vec<Value>>(),
                "charts": charted,
                "chart_list": Value::Array(charts),
            }));
            bump(&mut totals, "charts", charted);
            bump(&mut totals, "cells", one.cells.len());
            bump(&mut totals, "formulas", one.formulas());
            bump(&mut totals, "dates", one.date_cells());
            bump(&mut totals, "merged", one.merged);
            bump(&mut totals, "covered", one.covered);
            bump(&mut totals, "hidden_rows", one.hidden_rows);
            bump(&mut totals, "hidden_cols", one.hidden_cols);
            bump(&mut totals, "comments", one.comments.len());
        }
        let cut = book
            .sheets
            .iter()
            .any(|one| one.cells.len() > limit)
            .then(|| "格子只列前 --limit 个")
            .unwrap_or_default();
        if !cut.is_empty() {
            notes.push(cut.to_string());
        }
        // ODF 的表保护写在 `table:table` 自己身上的属性里（不像 OOXML 那样另有一层）
        let mut locks: Vec<Value> = Vec::new();
        if let Some(member) = xml(bytes, "content.xml") {
            let content_root = xmlscan::parse_str(&member.as_text());
            for one in content_root.descendants("table") {
                locks.push(crate::protect::ods_table(
                    one,
                    crate::odsheet::attr_of(one, "name").unwrap_or_default(),
                ));
            }
        }
        let result = json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "opendocument-spreadsheet",
            "workbook": {
                "sheets": book.sheets.len(),
                "hidden_sheets": book.sheets.iter().filter(|one| !one.visible).count(),
                "cell_total": book.cell_total(),
                "styles": {
                    "cell_styles": styles.counts().0,
                    "data_styles": styles.counts().1,
                },
                "totals": totals,
            },
            "sheets": sheets,
            // 页版式（母版页）那六格：.ods 的页眉页脚住在这儿，而**没有任何一条写着的属性**把某张表连到某份页版式
            "page_styles": crate::office_doc::odf_page_styles(bytes, limit),
            // 同一问在这一族是表自己身上的一条属性，另有一份为与 Excel 来回留的 named-*
            "print_ranges": crate::print_ranges::ods(bytes, limit),
            // 同一问在这一族是格子身上的一个属性，每条都带正文（没有共享组那一层）
            "formula_elems": crate::formula_elems::ods(bytes, limit),
            "protection": json!({
                "sheets": locks,
                "workbook": json!({"element": false}),
            }),
            "csv": csv_report(app.csv, &grid_names, &grids, &app.sheet, 0),
            "notes": notes,
        });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    if doc.family == Family::Compound && doc.app == "excel" {
        let cfb = doc
            .compound
            .as_ref()
            .ok_or_else(|| AppError::InvalidInput("复合文档打不开".to_string()))?;
        let book = crate::biff::read(cfb, bytes).map_err(AppError::InvalidInput)?;
        let mut notes = book.notes.clone();
        notes.push(
            "批注在这一族是第四种存法：不在另一个部件里，也不在格子里面，而在**同一条流**\
             的两类记录上 —— 一条给字（正文偏移 10 自报字数，紧跟的第一条 CONTINUE 首字节\
             说编码：0 是一格一字节、1 是一格两字节，这一位与 BIFF8 那个 fCompressed 的惯例\
             相反），一条给「哪个格子 + 谁写的」（住在这张表子流的末尾）。两类按出现顺序配，\
             所以每条都带 whole 说两边自报的字数是不是都正好切出来。这一族不写作者时间，\
             date 一律 null。这套读法是在 LibreOffice 写的 .xls 上量的，记录名也不替它们编"
                .to_string(),
        );
        let grid_names: Vec<String> = book.sheets.iter().map(|one| one.name.clone()).collect();
        let grids: Vec<Vec<(usize, usize, String)>> = book
            .sheets
            .iter()
            .map(|sheet| {
                book.cells
                    .iter()
                    .filter(|had| had.sheet.as_deref() == Some(sheet.name.as_str()))
                    .filter_map(|had| {
                        split_ref(&had.reference()).map(|(row, col)| (row, col, book.shown(had)))
                    })
                    .collect()
            })
            .collect();
        let cells: Vec<Value> = book
            .cells
            .iter()
            .take(limit)
            .map(|one| {
                merge(
                    json!({
                        "ref": one.reference(),
                        "kind": one.kind,
                        "sheet": one.sheet,
                        "text": one.text,
                        "number": one.number,
                    }),
                    book.cell_format(one).unwrap_or(Value::Null),
                )
            })
            .collect();
        // 这一族的锁写在**被锁那张表自己的子流**里，不在工作簿那层：与 xlsx 的两层、
        // ODF 的属性各是一种写法，所以这里按表交账
        let locks: Vec<Value> = book
            .sheets
            .iter()
            .map(|one| crate::protect::xls_sheet(&one.name, &one.protection))
            .collect();
        if book.sheets.iter().any(|one| !one.protection.is_empty()) {
            notes.push(
                "这一族没有「整本工作簿一层」的表锁：那几条记录住在表自己的子流里，\
                 锁哪张记在哪张名下（与 xlsx 的 workbookProtection + sheetProtection 两层不同）"
                    .to_string(),
            );
        }
        let result = json!({
            "path": app.path.to_string_lossy(),
            "format": doc.format,
            "kind": "biff8",
            "workbook": {
                "sheets": book.sheets.len(),
                "hidden_sheets": book.sheets.iter().filter(|one| one.state != "visible").count(),
                "shared_strings": book.strings.len(),
                "bofs": book.bofs.len(),
                "records": book.records,
                // 数字格式那一跳的两张表：XF 表只列格式号，FORMAT 表只有自定义号有串
                "date1904": book.date1904,
                "xfs": book.xfs,
                "formats": book.formats,
                "totals": {
                    "cells": book.cells.len(),
                    "formulas": book.formula_cells,
                    "hidden_rows": book.sheets.iter().map(|one| one.hidden_rows.len()).sum::<usize>(),
                    "hidden_cols": book.sheets.iter().map(|one| one.hidden_cols.len()).sum::<usize>(),
                    "comments": book.sheets.iter().map(|one| one.comments.len()).sum::<usize>(),
                },
            },
            "protection": json!({"kind": "biff8", "sheets": locks}),
            "sheets": book.sheets.iter().map(|one| json!({
                "name": one.name,
                "state": one.state,
                "record_start": one.record_start,
                // 隐藏的是「看不见」，那些格子照样算在 cells 里；只报条数，
                // 与 xlsx / ods 那两支同一个形状（位置在 biff 的单测里逐条断言）
                "hidden_rows": one.hidden_rows.len(),
                "hidden_cols": one.hidden_cols.len(),
                "comments": one.comments.len(),
                "comment_list": one.comments.iter().take(limit).map(|had| json!({
                    "ref": had.reference,
                    "author": had.author,
                    "date": Value::Null,
                    "text": had.text,
                    "whole": had.whole,
                })).collect::<Vec<Value>>(),
                // 两类记录各自有几条：配的条数只到小的那个，所以对不上时这里看得见
                "note_text_records": one.note_text_records,
                "note_cell_records": one.note_cell_records,
                "cells": book.cells.iter().filter(|had| had.sheet.as_deref() == Some(one.name.as_str())).count(),
            })).collect::<Vec<Value>>(),
            "cells": cells,
            "csv": csv_report(app.csv, &grid_names, &grids, &app.sheet, 0),
            "notes": notes,
        });
        ctx.done(result.clone(), start.elapsed().as_millis() as u64);
        return Ok(result);
    }
    Err(AppError::InvalidInput(format!(
        "{} 不是电子表格（识别为 {} / {}）；文档用 office-doc，演示文稿用 office-slide",
        app.path.display(),
        doc.app,
        doc.format
    )))
}

/// OOXML 的打印那份设置：每张表自己一份，**元素不在就是 null**。
/// 量过的两个生产者正好相反：openpyxl 只写 `pageMargins`，`pageSetup` 与 `printOptions`
/// 整个不存在（那些开关是「没说」，不是 false）；LibreOffice 重写同一份东西把十二个
/// `pageSetup` 属性全写出来，连 `paperSize="9"` 与两个 dpi 都不省
fn xlsx_print_setup(sheet_root: &xmlscan::Node) -> Value {
    let written = |name: &str| -> Value {
        let Some(node) = sheet_root.descendants(name).into_iter().next() else {
            return Value::Null;
        };
        written_attrs(node)
    };
    json!({
        // 边距在这一族是「英寸的浮点串」，照文件写的交出去，不换算成 0.01mm：
        // 0.5 与 0.511811023622047 正是两个生产者的差，换成整数就抹平了
        "margins": written("pageMargins"),
        "setup": written("pageSetup"),
        "options": written("printOptions"),
        "margin_unit": "inch",
    })
}

/// 这一张表的窗口状态：`sheetView` 写了哪些开关、有没有 `pane`、几条 `selection`
///
/// 同一个开关两家拼法不同（openpyxl 写 `showGridLines="0"`，LibreOffice 写 `"false"`
/// 并且把十几个属性全补出来），所以属性照文件交，不折成同一个布尔；
/// 「冻住」与「拆分」也在同一个 `state` 上分开，实测 LibreOffice 的 xlsx 导出
/// 会把 `state="split"` 那个 pane **整个丢掉**（同一份件冻住的那张留着）
fn xlsx_view(sheet_root: &xmlscan::Node, limit: usize) -> Value {
    let views = sheet_root.descendants("sheetView");
    let count = views.len();
    let first = views.into_iter().next();
    let pane = sheet_root.descendants("pane").into_iter().next();
    let selections: Vec<Value> = sheet_root
        .descendants("selection")
        .into_iter()
        .take(limit)
        .map(written_attrs)
        .collect();
    json!({
        "written": first.map(written_attrs).unwrap_or(Value::Null),
        "count": count,
        "pane": pane.map(written_attrs).unwrap_or(Value::Null),
        "selections": selections,
    })
}

/// 页眉页脚那六个段落元素，固定顺序交（缺哪个由每条的 `present` 说）
const HF_SLOTS: [&str; 6] = [
    "oddHeader",
    "oddFooter",
    "evenHeader",
    "evenFooter",
    "firstHeader",
    "firstFooter",
];

/// 页眉页脚串里的 `&` 码：只有 `&L` / `&C` / `&R` 是分段标记，别的一律原样列出来
///
/// 按码位扫而不是按字符数：`&"Calibri"` 里面带引号（LibreOffice 每段前面补一个，
/// openpyxl 一个不写），`&&` 是一个货真价实的 `&` 而不是标记，末尾落单的 `&` 什么都不接
fn hf_scan(text: &str) -> (Vec<Value>, Vec<String>) {
    let chars: Vec<char> = text.chars().collect();
    let mut marks: Vec<(&'static str, usize, usize)> = Vec::new();
    let mut fields: Vec<String> = Vec::new();
    let mut at = 0usize;
    while at < chars.len() {
        if chars[at] != '&' {
            at += 1;
            continue;
        }
        let Some(next) = chars.get(at + 1).copied() else {
            fields.push("&".to_string());
            at += 1;
            continue;
        };
        match next {
            '&' => {
                fields.push("&&".to_string());
                at += 2;
            }
            '"' => {
                let mut stop = at + 2;
                while stop < chars.len() && chars[stop] != '"' {
                    stop += 1;
                }
                if stop >= chars.len() {
                    stop = chars.len() - 1;
                }
                fields.push(chars[at..=stop].iter().collect());
                at = stop + 1;
            }
            'L' | 'C' | 'R' => {
                let name = match next {
                    'L' => "left",
                    'C' => "center",
                    _ => "right",
                };
                marks.push((name, at, at + 2));
                at += 2;
            }
            other => {
                fields.push(format!("&{}", other));
                at += 2;
            }
        }
    }
    let mut segments: Vec<Value> = Vec::new();
    for index in 0..marks.len() {
        let stop = marks.get(index + 1).map(|one| one.1).unwrap_or(chars.len());
        segments.push(json!({
            "at": marks[index].0,
            "text": chars[marks[index].2..stop].iter().collect::<String>(),
        }));
    }
    (segments, fields)
}

/// 这一张表的页眉页脚：六个段落固定交，`present` 说清元素在不在
///
/// 「没写这个元素」与「写了但是空的」是两件事：openpyxl 第三张表干脆不写
/// `headerFooter`（present 全 false、text 全 null），LibreOffice 六个都写出来而里面是空的
/// （present true、text ""）
fn xlsx_header_footer(sheet_root: &xmlscan::Node) -> Value {
    let holder = sheet_root.descendants("headerFooter").into_iter().next();
    let mut slots: Vec<Value> = Vec::new();
    let mut written_slots = 0usize;
    for name in HF_SLOTS {
        let found = sheet_root.descendants(name).into_iter().next();
        match found {
            None => slots.push(json!({
                "element": name,
                "present": false,
                "text": Value::Null,
                "segments": [],
                "fields": [],
            })),
            Some(one) => {
                let text = one.text();
                let (segments, fields) = hf_scan(&text);
                if !text.is_empty() {
                    written_slots += 1;
                }
                slots.push(json!({
                    "element": name,
                    "present": true,
                    "text": text,
                    "segments": segments,
                    "fields": fields,
                }));
            }
        }
    }
    json!({
        "written": holder.map(written_attrs).unwrap_or(Value::Null),
        "present": holder.is_some(),
        "slots": slots,
        "written_slots": written_slots,
    })
}

/// 文件自己写的条数与实际数出来的条数对不对得上（没写那条算对得上，不算不一致）
fn count_matches(written: &Option<String>, found: usize) -> bool {
    match written {
        None => true,
        Some(raw) => raw.trim().parse::<usize>().ok() == Some(found),
    }
}

/// 这一张表的尺寸账：`sheetFormatPr`、每一条 `col`、说过话的每一行
///
/// 宽度与高度都按文件写的字符串交：同一列在一家是 `22.5`、在另一家是 `20.47`，同一行
/// 一家写 `40`、另一家写 `39.75`，而「默认列宽」两家根本写的不是同一个属性
/// （`baseColWidth` 与 `defaultColWidth`）—— 折成一个数就是替文件编东西。
/// 一条 `col` 可以顶很多列（`min` 与 `max`），所以「几条」与「盖住几列」分开交、不展开
fn xlsx_layout(sheet_root: &xmlscan::Node, limit: usize) -> Value {
    let format = sheet_root.descendants("sheetFormatPr").into_iter().next();
    let cols = sheet_root.descendants("col");
    let mut covered = 0usize;
    let mut exact = !cols.is_empty();
    let col_total = cols.len();
    for one in cols.iter() {
        let low = one
            .attr("min")
            .and_then(|raw| raw.trim().parse::<usize>().ok());
        let high = one
            .attr("max")
            .and_then(|raw| raw.trim().parse::<usize>().ok());
        match (low, high) {
            (Some(low), Some(high)) if high >= low => covered += high - low + 1,
            _ => exact = false,
        }
    }
    let rows = sheet_root.descendants("row");
    let row_total = rows.len();
    let mut with_height = 0usize;
    for one in rows.iter() {
        if one.attr("ht").is_some() {
            with_height += 1;
        }
    }
    let tall: Vec<&xmlscan::Node> = rows
        .into_iter()
        .filter(|one| {
            one.attr("ht").is_some()
                || one.attr("customHeight").is_some()
                || one.attr("hidden").is_some()
        })
        .collect();
    let spoken = tall.len();
    let row_list: Vec<Value> = tall.into_iter().take(limit).map(written_attrs).collect();
    let col_list: Vec<Value> = cols.into_iter().take(limit).map(written_attrs).collect();
    json!({
        "format": format.map(written_attrs).unwrap_or(Value::Null),
        "columns": {
            "written": col_total,
            "covered": if exact { json!(covered) } else { Value::Null },
            "list": col_list,
        },
        "rows": {
            "elements": row_total,
            "with_height": with_height,
            "spoken": spoken,
            "list": row_list,
        },
    })
}

/// ODF 的一条轴（列或行）：总账 + 逐条账本。这一族的尺寸**不在元素上** —— 元素只写
/// `number-*-repeated` 和偶尔一句 `table:visibility`，宽高在它点名的那份自动样式里，
/// 所以逐条都得说清「点了谁」「那份样式找着没找着」。
/// 三个「看见多少」各是各的：`elements`/`spans` 按全部元素加，`listed` 是账本存下的条数
/// （读的一边有上限），`shown` 是这次交出来的条数（受 --limit 管）
fn ods_axis(
    had: &crate::odsheet::SizeTally,
    items: &[crate::odsheet::SizeInfo],
    limit: usize,
) -> Value {
    let shown = items.len().min(limit);
    json!({
        "elements": had.elements,
        "spans": had.spans,
        "listed": items.len(),
        "shown": shown,
        "resolved": had.resolved,
        "with_size": had.with_size,
        "optimal": had.optimal,
        "spoken_visibility": had.spoken_visibility,
        "list": items
            .iter()
            .take(shown)
            .map(|one| json!({
                "style": one.style,
                "repeated": one.repeated,
                "element_visibility": one.element_visibility,
                "size": one.size,
                "size_mm": one.size.as_deref().and_then(|raw| crate::paper::length(raw)),
                "optimal": one.optimal,
                "style_visibility": one.style_visibility,
                "style_parent": one.style_parent,
                "resolved": one.resolved,
            }))
            .collect::<Vec<Value>>(),
    })
}

/// 一张 ODF 表的布局：两条轴各一份账，外加表元素自己写的那四个数（没写的交 null，
/// 而「没写」与「写了 0」不是一回事）。`office-slide` 的 odp 分支交的是同一份账，
/// 所以这里给整个 crate 用（一张 .ods 的表与一页 odp 上的表是同一种元素）
pub(crate) fn ods_layout(one: &crate::odsheet::Sheet, limit: usize) -> Value {
    let mut stated = serde_json::Map::new();
    for (name, raw) in one.stated.iter() {
        stated.insert(
            name.clone(),
            raw.as_ref().map(|had| json!(had)).unwrap_or(Value::Null),
        );
    }
    json!({
        "unit": crate::paper::UNIT,
        "columns": ods_axis(&one.col_tally, &one.col_sizes, limit),
        "rows": ods_axis(&one.row_tally, &one.row_sizes, limit),
        "stated": Value::Object(stated),
    })
}

/// 这一张表的筛选：`autoFilter` 在不在、范围、哪几列在筛，另附 `sheetPr` 上那个 filterMode
///
/// `filterColumn` 上的 `hiddenButton` / `showButton` 只有一家写，`filters` 上的 `blank` 也是；
/// 筛掉之后这一族会不会另写一条 `sheetPr filterMode="true"`，两家也不一样
fn xlsx_filter(sheet_root: &xmlscan::Node, limit: usize) -> Value {
    let holder = sheet_root.descendants("autoFilter").into_iter().next();
    let sheet_pr = sheet_root.descendants("sheetPr").into_iter().next();
    let mut columns: Vec<Value> = Vec::new();
    for one in sheet_root
        .descendants("filterColumn")
        .into_iter()
        .take(limit)
    {
        let mut kinds: Vec<String> = Vec::new();
        let mut vals: Vec<Value> = Vec::new();
        for kid in &one.children {
            if kid.local() != "filters" {
                continue;
            }
            kinds.push("filters".to_string());
            for deep in &kid.children {
                if deep.local() == "filter" {
                    vals.push(deep.attr("val").map(Value::from).unwrap_or(Value::Null));
                } else {
                    kinds.push(format!("filters/{}", deep.local()));
                }
            }
        }
        columns.push(json!({
            "written": written_attrs(one),
            "kinds": kinds,
            "vals": vals,
        }));
    }
    json!({
        "present": holder.is_some(),
        "written": holder.map(written_attrs).unwrap_or(Value::Null),
        "mode": sheet_pr
            .and_then(|one| one.attr("filterMode"))
            .map(|raw| json!(raw))
            .unwrap_or(Value::Null),
        "columns": columns,
    })
}

/// `tableParts` 指着的那个表对象部件：属性照交，列名单独列出（名字是文件自己写的）
fn table_one(bytes: &[u8], name: &str, limit: usize) -> Value {
    let Some(member) = xml(bytes, name) else {
        return json!({"part": name, "present": false});
    };
    let root = xmlscan::parse_str(&member.as_text());
    let holder = root.descendants("table").into_iter().next();
    let columns = root.descendants("tableColumn");
    let counted = root.descendants("tableColumns").into_iter().next();
    let written = counted.and_then(|one| one.attr("count")).map(String::from);
    let found = columns.len();
    let names: Vec<Value> = columns
        .iter()
        .take(limit)
        .map(|one| one.attr("name").map(Value::from).unwrap_or(Value::Null))
        .collect();
    let list: Vec<Value> = columns.into_iter().take(limit).map(written_attrs).collect();
    let inner = root.descendants("autoFilter").into_iter().next();
    let style = root.descendants("tableStyleInfo").into_iter().next();
    json!({
        "part": name,
        "present": true,
        "written": holder.map(written_attrs).unwrap_or(Value::Null),
        "columns": {
            "written": written.clone(),
            "found": found,
            "whole": count_matches(&written, found),
            "names": names,
            "list": list,
        },
        "filter": {
            "present": inner.is_some(),
            "written": inner.map(written_attrs).unwrap_or(Value::Null),
        },
        "style": style.map(written_attrs).unwrap_or(Value::Null),
    })
}

/// 这一张表挂上的表对象：`tableParts` 自报的数与实际条数并排，部件顺着这张表自己的关系表找
fn sheet_tables(bytes: &[u8], part: &str, sheet_root: &xmlscan::Node, limit: usize) -> Value {
    let holder = sheet_root.descendants("tableParts").into_iter().next();
    let written = holder.and_then(|one| one.attr("count")).map(String::from);
    let found = sheet_root.descendants("tablePart").len();
    let list: Vec<Value> = rels_of(bytes, part)
        .into_iter()
        .filter(|(kind, _)| kind == "table")
        .take(limit)
        .map(|(_, target)| table_one(bytes, &target, limit))
        .collect();
    json!({
        "tables": list.len(),
        "table_list": list,
        "parts": {
            "written": written.clone(),
            "found": found,
            "whole": count_matches(&written, found),
            "resolved": list.len(),
        },
    })
}

/// 一个元素上写着的属性，按局部名原样交出去（名字去掉前缀，值不做任何解释）
///
/// 命名空间声明不算属性：`xmlns=` 与 `xmlns:fo=` 是「这一族怎么读这个名字」的声明，
/// 不是这个元素携带的数据，标准库的 XML 读者也不把它放进 attrib —— 两边同一口径，
/// 否则部件根元素那份账会凭空多出一条谁都没当它是值的串
pub(crate) fn written_attrs(node: &xmlscan::Node) -> Value {
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

/// `xl/styles.xml` 里 `dxfs` 那一跳：条件格式的规则只写一个下标（`dxfId`），真正的字色
/// 住在这里。`count` 自报的数与实际条数一起交；每条 dxf 交它里面出现的元素路径
/// （`font/b` 这种一层到底的写法）—— openpyxl 那一份写两个元素，LibreOffice 重写同一份
/// 东西给五个（多出的 `name` / `family` / `sz` 是它自己补的），所以只交出现过的名字，
/// 不替两边凑成「加粗的深红」那种共同的形状
fn dxf_table(bytes: &[u8]) -> (Option<String>, Vec<Vec<String>>) {
    let Some(member) = xml(bytes, "xl/styles.xml") else {
        return (None, Vec::new());
    };
    let root = xmlscan::parse_str(&member.as_text());
    let holder = root.descendants("dxfs").into_iter().next();
    let written = holder.and_then(|one| one.attr("count")).map(String::from);
    let kinds: Vec<Vec<String>> = holder
        .iter()
        .flat_map(|one| one.children.iter())
        .filter(|one| one.local() == "dxf")
        .map(|one| {
            let mut out: Vec<String> = Vec::new();
            for kid in &one.children {
                if kid.children.is_empty() {
                    out.push(kid.local().to_string());
                } else {
                    for deep in &kid.children {
                        out.push(format!("{}/{}", kid.local(), deep.local()));
                    }
                }
            }
            out
        })
        .collect();
    (written, kinds)
}

/// 这一张表上的规则：条件格式（`conditionalFormatting` 一块一套范围，里面若干条 `cfRule`）
/// 与数据验证（`dataValidations` 那个容器自报 `count`）。两边的属性全部按文件写的交，
/// 只把 `type` / `priority` / `operator` / `sqref` 另外点名，因为断言要看的就是这几个
fn sheet_rules(sheet_root: &xmlscan::Node, dxfs: &[Vec<String>], limit: usize) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    for block in sheet_root.descendants("conditionalFormatting") {
        let mut rules: Vec<Value> = Vec::new();
        for rule in block
            .children
            .iter()
            .filter(|one| one.local() == "cfRule")
            .take(limit)
        {
            let index = rule
                .attr("dxfId")
                .and_then(|raw| raw.trim().parse::<usize>().ok());
            let dxf = match (rule.attr("dxfId"), index.and_then(|at| dxfs.get(at))) {
                (Some(raw), Some(kinds)) => json!({"written": raw, "found": true, "kinds": kinds}),
                (Some(raw), None) => {
                    json!({"written": raw, "found": false, "kinds": []})
                }
                (None, _) => json!({"written": Value::Null, "found": false, "kinds": []}),
            };
            // 图标集与色阶那两种把形状写在子元素里（`cfvo` 的 type/val 与 color 的 rgb）
            let detail = rule
                .children
                .iter()
                .find(|one| matches!(one.local(), "iconSet" | "colorScale" | "dataBar" | "extLst"));
            let scale = match detail {
                Some(one) if one.local() != "extLst" => json!({
                    "kind": one.local(),
                    "written": written_attrs(one),
                    "cfvo": one.children.iter().filter(|had| had.local() == "cfvo")
                        .map(written_attrs).collect::<Vec<Value>>(),
                    "colors": one.children.iter().filter(|had| had.local() == "color")
                        .filter_map(|had| had.attr("rgb")).map(String::from)
                        .collect::<Vec<String>>(),
                }),
                _ => Value::Null,
            };
            rules.push(json!({
                "type": rule.attr("type"),
                "priority": rule.attr("priority"),
                "operator": rule.attr("operator"),
                "dxf": dxf,
                "formulas": rule.children.iter().filter(|one| one.local() == "formula")
                    .map(|one| one.text().trim().to_string()).collect::<Vec<String>>(),
                "scale": scale,
                "written": written_attrs(rule),
            }));
        }
        blocks.push(json!({
            "sqref": block.attr("sqref"),
            "rules": rules.len(),
            "rule_list": rules,
        }));
    }
    let holder = sheet_root.descendants("dataValidations").into_iter().next();
    let list: Vec<Value> = holder
        .iter()
        .flat_map(|one| one.children.iter())
        .filter(|one| one.local() == "dataValidation")
        .take(limit)
        .map(|one| {
            json!({
                "sqref": one.attr("sqref"),
                "type": one.attr("type"),
                "operator": one.attr("operator"),
                "formulas": one.children.iter()
                    .filter(|had| matches!(had.local(), "formula1" | "formula2"))
                    .map(|had| had.text().trim().to_string()).collect::<Vec<String>>(),
                "written": written_attrs(one),
            })
        })
        .collect();
    let written = holder.and_then(|one| one.attr("count")).map(String::from);
    let found = list.len();
    let whole = match &written {
        None => true,
        Some(raw) => raw.trim().parse::<usize>().ok() == Some(found),
    };
    json!({
        "conditional": blocks,
        "validations": {
            "written": written,
            "found": found,
            "whole": whole,
            "list": list,
        },
    })
}

/// 这张表的批注。它们**不在** `sheetN.xml` 里：要靠这张表自己的关系表
/// （`xl/worksheets/_rels/sheet1.xml.rels`，Type 结尾是 `/comments` 那一条）
/// 跳到批注部件。两个生产者把那个部件放在两个地方 —— openpyxl 写
/// `xl/comments/comment1.xml`（Target 还是绝对的 `/xl/...`），LibreOffice 写
/// `xl/comments1.xml`（Target 是相对的 `../comments1.xml`）—— `resolve_target` 两种都吃。
/// 作者名也不在格子上，只有一个 `authorId` 下标，指向同一部件开头 `<authors>` 那个列表
fn sheet_comments(bytes: &[u8], part: &str, limit: usize) -> Vec<Value> {
    let dir = match part.rsplit_once('/') {
        Some((head, _)) => head.to_string(),
        None => String::new(),
    };
    let base = part.rsplit('/').next().unwrap_or(part);
    let rels_name = if dir.is_empty() {
        format!("_rels/{base}.rels")
    } else {
        format!("{dir}/_rels/{base}.rels")
    };
    let Some(rels) = xml(bytes, &rels_name) else {
        return Vec::new();
    };
    let rel_root = xmlscan::parse_str(&rels.as_text());
    let Some(target) = rel_root
        .descendants("Relationship")
        .iter()
        .filter(|one| one.attr("TargetMode") != Some("External"))
        .find(|one| one.attr("Type").unwrap_or_default().ends_with("/comments"))
        .and_then(|one| one.attr("Target"))
    else {
        return Vec::new();
    };
    let located = resolve_target(&dir, target);
    let Some(member) = xml(bytes, &located) else {
        return Vec::new();
    };
    let root = xmlscan::parse_str(&member.as_text());
    let authors: Vec<String> = root
        .descendants("author")
        .iter()
        .map(|one| one.text().trim().to_string())
        .collect();
    let mut out: Vec<Value> = Vec::new();
    for one in root.descendants("comment").iter() {
        let index = one
            .attr("authorId")
            .and_then(|had| had.parse::<usize>().ok())
            .unwrap_or(usize::MAX);
        out.push(json!({
            "ref": one.attr("ref"),
            "author": authors.get(index).cloned(),
            "date": one.attr("date"),
            "text": one
                .descendants("t")
                .iter()
                .map(|had| had.text())
                .collect::<Vec<String>>()
                .join(""),
        }));
        if out.len() >= limit {
            break;
        }
    }
    out
}

/// 能当数看的就当数给出：xlsx 的 `<v>` 与 .xls 的 RK 是同一个数，别一个报字串一个报浮点
fn numeric_or_text(text: Option<String>) -> Value {
    match text {
        Some(one) => match one.parse::<f64>() {
            Ok(raw) => json!(raw),
            Err(_) => json!(one),
        },
        None => Value::Null,
    }
}

fn xml(bytes: &[u8], want: &str) -> Option<zipread::Member> {
    zipread::member(bytes, want, DEFAULT_MEMBER_CAP).ok()
}

/// 这个部件自己的关系表：交回「Type 结尾那个名字」与「解析成包内全名的 Target」。
/// 批注、图这些都从这一张表上跳；两种生产者的 Target 写法不同（绝对的 `/xl/...`
/// 与相对的 `../...`）由 `resolve_target` 吃掉
pub(crate) fn rels_of(bytes: &[u8], part: &str) -> Vec<(String, String)> {
    let dir = match part.rsplit_once('/') {
        Some((head, _)) => head.to_string(),
        None => String::new(),
    };
    let base = part.rsplit('/').next().unwrap_or(part);
    let rels_name = if dir.is_empty() {
        format!("_rels/{base}.rels")
    } else {
        format!("{dir}/_rels/{base}.rels")
    };
    let Some(rels) = xml(bytes, &rels_name) else {
        return Vec::new();
    };
    let root = xmlscan::parse_str(&rels.as_text());
    root.descendants("Relationship")
        .iter()
        .filter(|one| one.attr("TargetMode") != Some("External"))
        .filter_map(|one| {
            let kind = one.attr("Type").unwrap_or_default();
            let tail = kind.rsplit('/').next().unwrap_or(kind).to_string();
            let target = one.attr("Target")?;
            Some((tail, resolve_target(&dir, target)))
        })
        .collect()
}

/// 那张表上的图：要跳三跳 —— 表 →（自己的关系表）→ 画法部件 →（它的关系表）→ 图部件。
/// 两个生产者三种 Target 写法都在样本里（`/xl/drawings/…`、`../drawings/…`、`../charts/…`）
fn sheet_charts(bytes: &[u8], part: &str, limit: usize) -> Vec<Value> {
    let mut out: Vec<Value> = Vec::new();
    for (_, drawing) in rels_of(bytes, part) {
        if !drawing.ends_with(".xml") || !drawing.contains("/drawings/") {
            continue;
        }
        for (kind, target) in rels_of(bytes, &drawing) {
            if kind != "chart" || !target.contains("/charts/") {
                continue;
            }
            let Some(member) = xml(bytes, &target) else {
                continue;
            };
            let root = xmlscan::parse_str(&member.as_text());
            out.push(chart_one(&root, &target));
            if out.len() >= limit {
                return out;
            }
        }
    }
    out
}

/// 一个引用（`c:tx` / `c:cat` / `c:val` 里面那一个）。走法只有三种：`strRef`、`numRef`
/// 与 openpyxl 那种没有引用的 `rich`；引用串**照文件写的交**（`'数据'!B1` 与 `数据!$B$1`
/// 是两个生产者对同一段格子的两种写法，替它们归一化就是替文件编东西）。
/// 缓存那份另说：openpyxl 一个字都不写（于是「图里画的是哪些数」判不住），
/// LibreOffice 写 `ptCount` 自报的点数与实际点数一起交，对不上就看得见
pub(crate) fn chart_ref(node: &xmlscan::Node) -> Value {
    let Some(one) = node
        .children
        .iter()
        .find(|had| had.local().ends_with("Ref"))
    else {
        let joined: String = node
            .descendants("t")
            .iter()
            .map(|had| had.text())
            .collect::<Vec<String>>()
            .join("");
        let text = joined.trim().to_string();
        return json!({
            "via": if text.is_empty() { Value::Null } else { json!("text") },
            "ref": Value::Null,
            "text": if text.is_empty() { Value::Null } else { json!(text) },
            "cache": null_cache(),
        });
    };
    let reference = one
        .descendants("f")
        .first()
        .map(|had| had.text().trim().to_string());
    let cache = one
        .descendants("ptCount")
        .first()
        .and_then(|had| had.attr("val"));
    let points: Vec<Value> = one
        .descendants("pt")
        .iter()
        .map(|had| {
            had.child("v")
                .map(|inner| json!(numeric_or_text(Some(inner.text().trim().to_string()))))
                .unwrap_or(Value::Null)
        })
        .collect();
    let written = cache.map(|raw| raw.trim().to_string());
    let whole = match &written {
        None => true,
        Some(raw) => raw.parse::<usize>().ok() == Some(points.len()),
    };
    json!({
        "via": one.local(),
        "ref": reference,
        "text": Value::Null,
        "cache": {"written": written, "points": points.len(), "values": points, "whole": whole},
    })
}

fn null_cache() -> Value {
    json!({"written": Value::Null, "points": 0usize, "values": [], "whole": true})
}

/// 整个引用不在（这张图没写 `c:cat`，或标题是个空 `c:title`）：形状与「写了引用但
/// 没有缓存」要能分开，所以 via/ref/text 三个都是 null，而不是缺键
fn no_ref() -> Value {
    json!({"via": Value::Null, "ref": Value::Null, "text": Value::Null, "cache": null_cache()})
}

/// 一张图：类型那一组（`barChart` 与 `lineChart` 这些）、标题的两种写法、每条系列
pub(crate) fn chart_one(root: &xmlscan::Node, part: &str) -> Value {
    let Some(chart) = root.descendants("chart").first().copied() else {
        return json!({"part": part, "present": false});
    };
    // 标题的三种写法都从 `c:title/c:tx` 那一个元素上分：`c:strRef`（引用一个格子）、
    // `c:rich`（字面量，两个生产者目前都走这条）与整个 title 不在
    let title = match chart.child("title").and_then(|one| one.child("tx")) {
        Some(tx) => chart_ref(tx),
        None => match chart.child("title") {
            Some(node) => chart_ref(node),
            None => no_ref(),
        },
    };
    let mut groups: Vec<Value> = Vec::new();
    let mut any_cached = false;
    for group in chart
        .child("plotArea")
        .into_iter()
        .flat_map(|one| one.children.iter())
        .filter(|one| one.local().ends_with("Chart"))
    {
        let mut written = serde_json::Map::new();
        let mut axis_ids: Vec<String> = Vec::new();
        let mut series: Vec<Value> = Vec::new();
        for one in &group.children {
            match one.local() {
                "ser" => series.push(json!({
                    "index": one.child("idx").and_then(|had| had.attr("val")).map(String::from),
                    "order": one.child("order").and_then(|had| had.attr("val")).map(String::from),
                    "name": one.child("tx").map(chart_ref).unwrap_or_else(no_ref),
                    "cat": one.child("cat").map(chart_ref).unwrap_or_else(no_ref),
                    "val": one.child("val").map(chart_ref).unwrap_or_else(no_ref),
                })),
                "axId" => {
                    if let Some(raw) = one.attr("val") {
                        axis_ids.push(raw.to_string());
                    }
                }
                _ => {
                    if let Some(raw) = one.attr("val") {
                        written.insert(one.local().to_string(), json!(raw));
                    }
                }
            }
        }
        // 「图里画的是哪些数」只有文件自己缓存过才算判得住：openpyxl 一条 pt 都不写
        any_cached |= series
            .iter()
            .any(|one| one["val"]["cache"]["points"].as_u64().unwrap_or(0) > 0);
        groups.push(json!({
            "kind": group.local(),
            "written": written,
            "axis_ids": axis_ids,
            "series": series.len(),
            "series_list": series,
        }));
    }
    json!({
        "part": part,
        "present": true,
        "title": title,
        "cached": any_cached,
        "groups": groups,
    })
}

/// 一条「串」（`si` 或 `is`）的三段账：整串的字、文件把它分成的段、每段自己写了什么。
///
/// 分段就是富文本：`<r>` 里那段字带着自己的 `rPr`（字号、粗体、颜色…），而直接坐在串下的
/// `<t>` 没有格式。两件事各交各的：`text` 是整串**按文件写的原样**（不 trim ——
/// `  两头有空格  ` 那两个空格是文件写的，LibreOffice 自己的 CSV 导出也带着它们），
/// `runs` 是分段本身。
#[derive(Clone)]
struct StringParts {
    /// 整串的字，原样
    text: String,
    /// 分段（`--limit` 之内的那几段）
    runs: Vec<Value>,
    /// 文件一共分了几段
    run_total: usize,
    /// 这条串里有 `r`（也就是真分了段）
    rich: bool,
    /// 有一个 `t` 写了 `xml:space`
    preserved: bool,
}

fn string_parts(holder: &xmlscan::Node, limit: usize) -> StringParts {
    let mut out = StringParts {
        text: holder.text(),
        runs: Vec::new(),
        run_total: 0,
        rich: false,
        preserved: false,
    };
    for one in &holder.children {
        let (word, props) = match one.local() {
            "t" => (one, None),
            "r" => {
                out.rich = true;
                (one.child("t").unwrap_or(one), one.child("rPr"))
            }
            _ => continue,
        };
        let space = word.attr("xml:space");
        if space.is_some() {
            out.preserved = true;
        }
        out.run_total += 1;
        if out.runs.len() >= limit {
            continue;
        }
        let mut attrs = serde_json::Map::new();
        let mut shape: Vec<Value> = Vec::new();
        if let Some(had) = props {
            for (key, value) in had.attrs.iter() {
                attrs.insert(key.clone(), json!(value));
            }
            // 那几家生产者把格式写在 **孩子元素** 上（`<b val="true"/>`、`<color rgb="…"/>`），
            // 而不是 rPr 自己的属性上 —— 两种都交，空的那个就说「这个文件没这么写」
            for kid in &had.children {
                let mut one = serde_json::Map::new();
                for (key, value) in kid.attrs.iter() {
                    one.insert(key.clone(), json!(value));
                }
                shape.push(json!({"element": kid.name, "attrs": Value::Object(one)}));
            }
        }
        out.runs.push(json!({
            "element": one.local(),
            "text": word.text(),
            "space": space,
            "props_written": props.is_some(),
            "props_attrs": if props.is_some() {
                Value::Object(attrs)
            } else {
                Value::Null
            },
            "format": if props.is_some() {
                Value::Array(shape)
            } else {
                Value::Null
            },
        }));
    }
    out
}

/// 共享字符串表：每一段的账，外加文件自己在那条 `sst` 上报的两个数
struct StringTable {
    entries: Vec<StringParts>,
    /// `sst/@count`（「引用了几次」），没写 null
    count_written: Option<String>,
    /// `sst/@uniqueCount`（「几条不重复」），没写 null
    unique_written: Option<String>,
}

fn shared_strings(bytes: &[u8], limit: usize) -> StringTable {
    let empty = || StringTable {
        entries: Vec::new(),
        count_written: None,
        unique_written: None,
    };
    let Some(member) = xml(bytes, "xl/sharedStrings.xml") else {
        return empty();
    };
    let root = xmlscan::parse_str(&member.as_text());
    let written = |want: &str| {
        root.descendants("sst")
            .first()
            .and_then(|one| one.attr(want))
            .map(|one| one.to_string())
    };
    StringTable {
        entries: root
            .descendants("si")
            .iter()
            .map(|one| string_parts(one, limit))
            .collect(),
        count_written: written("count"),
        unique_written: written("uniqueCount"),
    }
}

/// 一格最多铺进 CSV 的总数上限：坏文件可以自报几百万格，铺完就成了内存事故
const MAX_GRID_CELLS: usize = 200_000;

/// `hidden` 这个开关两种写法都有：openpyxl 写 `hidden="1"`，LibreOffice 写
/// `hidden="true"`（而且没隐藏的行也照样写 `hidden="false"`）
fn hidden_on(node: &xmlscan::Node) -> bool {
    matches!(
        node.attr("hidden")
            .unwrap_or_default()
            .to_ascii_lowercase()
            .as_str(),
        "1" | "true"
    )
}

/// `<col>` 是带跨度的：LibreOffice 把连续三列并成一条 `min="3" max="5"`，
/// 按元素个数数就会少报两列。只写一端就按一列算（另一端规范里默认与之同值）
fn col_span(node: &xmlscan::Node) -> usize {
    let min = node
        .attr("min")
        .and_then(|raw| raw.trim().parse::<usize>().ok());
    let max = node
        .attr("max")
        .and_then(|raw| raw.trim().parse::<usize>().ok());
    match (min, max) {
        (Some(first), Some(last)) if last >= first => last - first + 1,
        _ => 1,
    }
}

/// `B4` → (行 3, 列 1)。不是 A1 形状就交回 None —— 位置猜不出来就不铺。
fn split_ref(reference: &str) -> Option<(usize, usize)> {
    let raw = reference.trim();
    let split = raw.find(|ch: char| ch.is_ascii_digit())?;
    let letters = &raw[..split];
    let digits = &raw[split..];
    if letters.is_empty() || !digits.chars().all(|ch| ch.is_ascii_digit()) {
        return None;
    }
    let mut col = 0usize;
    for ch in letters.chars() {
        let letter = ch.to_ascii_uppercase();
        if !('A'..='Z').contains(&letter) {
            return None;
        }
        col = col.checked_mul(26)? + usize::from(letter as u8 - b'A') + 1;
    }
    let row: usize = digits.parse().ok()?;
    if row == 0 {
        return None;
    }
    Some((row - 1, col - 1))
}

/// CSV 里的一格：RFC4180 —— 带逗号、引号、换行就整体加引号，里面的引号翻倍
fn csv_field(raw: &str) -> String {
    if raw.contains(['"', ',', '\n', '\r']) {
        format!("\"{}\"", raw.replace('"', "\"\""))
    } else {
        raw.to_string()
    }
}

/// 把 (行, 列, 文本) 铺成一张网再拼成 CSV：空洞是空字段，尾部不裁（空格子也是格子）
fn render_csv(cells: &[(usize, usize, String)]) -> (String, usize, usize) {
    let rows = cells.iter().map(|one| one.0 + 1).max().unwrap_or(0);
    let cols = cells.iter().map(|one| one.1 + 1).max().unwrap_or(0);
    let mut grid: Vec<Vec<String>> = vec![vec![String::new(); cols]; rows];
    for (row, col, text) in cells {
        if *row < rows && *col < cols {
            grid[*row][*col] = text.clone();
        }
    }
    let mut out = String::new();
    for line in &grid {
        out.push_str(
            &line
                .iter()
                .map(|one| csv_field(one.as_str()))
                .collect::<Vec<String>>()
                .join(","),
        );
        out.push('\n');
    }
    (out, rows, cols)
}

/// ODS 一格进 CSV 该写什么：跟 xlsx 那条同一口径 —— **给值，不给显示格式**，
/// 日期给 ISO（ODF 本来就写着 ISO），布尔给 TRUE/FALSE，文本给文本。
/// 于是同一份数据在 xlsx 与 ods 两边拼出来的 CSV 是一样的，
/// 而不是 `12.5%` 与 `0.125` 各来一份。
fn ods_display(one: &crate::odsheet::Cell) -> String {
    if let Some(raw) = &one.date_value {
        return raw.clone();
    }
    match one.value_type.as_deref() {
        Some("boolean") => {
            if one.boolean_value.as_deref() == Some("true") {
                "TRUE".to_string()
            } else {
                "FALSE".to_string()
            }
        }
        Some("float") | Some("percentage") | Some("currency") => one
            .value
            .as_ref()
            .and_then(|raw| raw.parse::<f64>().ok())
            .map(|value| format!("{value}"))
            .unwrap_or_else(|| one.text.clone()),
        _ => one.text.clone(),
    }
}

/// `--sheet` 说的是序号还是表名：先试序号（命令报的 `index` 从 0 起），再按表名找
fn pick_sheet(names: &[String], want: &str) -> Option<usize> {
    let want = want.trim();
    if want.is_empty() {
        return if names.is_empty() { None } else { Some(0) };
    }
    if let Some(index) = want.parse::<usize>().ok().filter(|one| *one < names.len()) {
        return Some(index);
    }
    names.iter().position(|one| one == want)
}

/// `--csv` 的那份账：没开就整个键都不给（不给一份空 CSV 装作有）
fn csv_report(
    want: bool,
    names: &[String],
    grids: &[Vec<(usize, usize, String)>],
    pick: &str,
    skipped: usize,
) -> Value {
    if !want {
        return Value::Null;
    }
    let Some(index) = pick_sheet(names, pick) else {
        return json!({"error": format!("没有这张表「{}」；这份文件里的表是 {:?}", pick, names)});
    };
    let (text, rows, columns) =
        render_csv(grids.get(index).map(|one| &one[..]).unwrap_or_default());
    json!({
        "sheet": names[index],
        "index": index,
        "rows": rows,
        "columns": columns,
        "cells_skipped": skipped,
        "line_end": "LF",
        "text": text,
    })
}

/// 把格式账并到格子那条记录上（`cell_format` 交回的是一个小对象）
fn merge(base: Value, extra: Value) -> Value {
    let mut map = base.as_object().cloned().unwrap_or_default();
    if let Some(more) = extra.as_object() {
        for (key, value) in more {
            map.insert(key.clone(), value.clone());
        }
    }
    Value::Object(map)
}

fn bump(totals: &mut Value, key: &str, add: usize) {
    if let (Some(old), Some(map)) = (totals[key].as_u64(), totals.as_object_mut()) {
        map.insert(key.to_string(), json!(old + add as u64));
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use lilyco::prelude::Context;
    use std::sync::mpsc;

    fn run(name: &str) -> Value {
        let app = OfficeSheet {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit: 200,
            max_bytes: 1 << 26,
            csv: false,
            sheet: String::new(),
        };
        let (tx, _rx) = mpsc::channel();
        run_office_sheet(&app, &Context::new_test(tx)).expect("office-sheet 应成功")
    }

    /// 保护这份账分两层，还有两种拼法（期望值来自 `lyco_protect.py`）：
    /// openpyxl 写 `sheet="1" formatCells="0"`，LibreOffice 重写同一份东西写
    /// `sheet="true" formatCells="false"` 并把等于默认的开关省掉
    #[test]
    fn protection_is_accounted_for_at_both_levels() {
        let hand = run("locked-sheet.xlsx");
        let prot = &hand["protection"];
        assert_eq!(prot["workbook"]["element"], json!(true), "{prot}");
        assert_eq!(prot["workbook"]["lock_structure"], json!(true));
        assert_eq!(prot["workbook"]["book_password"], json!(true));
        let sheets = prot["sheets"].as_array().expect("是数组");
        assert_eq!(sheets.len(), 3, "{prot}");
        assert_eq!(sheets[0]["name"], "预算表");
        assert_eq!(sheets[0]["protected"], json!(true));
        assert_eq!(sheets[0]["written"]["formatCells"], json!(false));
        assert_eq!(sheets[0]["written"]["insertRows"], json!(true));
        assert_eq!(sheets[1]["element"], json!(false), "没写的表要说「没写」");
        assert_eq!(sheets[1]["protected"], json!(false));

        let lo = run("locked-sheet-lo.xlsx");
        let two = &lo["protection"];
        assert_eq!(
            two["sheets"][0]["protected"],
            json!(true),
            "两种拼法同一个结论"
        );
        assert_eq!(two["sheets"][0]["written"]["formatCells"], json!(false));
        assert_eq!(
            two["sheets"][0]["written"].get("insertRows"),
            None,
            "省掉的不补上"
        );
        // LibreOffice 导出 xlsx 时把结构锁丢成了一个空元素 —— 照实报，不替它补
        assert_eq!(two["workbook"]["element"], json!(true));
        assert_eq!(two["workbook"]["lock_structure"], Value::Null);
        assert_eq!(two["workbook"]["book_password"], json!(false));

        // openpyxl 本来就在 book.xlsx 里留了一个空的 workbookProtection：
        // 元素在场不等于锁上
        let plain = run("book.xlsx");
        assert_eq!(plain["protection"]["workbook"]["element"], json!(true));
        assert_eq!(
            plain["protection"]["workbook"]["lock_structure"],
            Value::Null
        );
        assert!(plain["protection"]["sheets"]
            .as_array()
            .expect("是数组")
            .iter()
            .all(|one| one["protected"] == json!(false)));
    }

    /// ODF 的表保护是 `table:table` 身上的属性，摘要那条 URI 只留最后一段
    #[test]
    fn opendocument_table_locks_come_off_the_table() {
        let out = run("locked-sheet.ods");
        let sheets = out["protection"]["sheets"].as_array().expect("是数组");
        assert_eq!(sheets.len(), 3, "{out}");
        assert_eq!(sheets[0]["name"], "预算表");
        assert_eq!(sheets[0]["protected"], json!(true));
        assert_eq!(
            sheets[0]["password"],
            json!(true),
            "table:protection-key 在"
        );
        assert_eq!(sheets[0]["digest"], json!("legacy-hash-excel"));
        assert_eq!(sheets[1]["protected"], json!(false));
        assert_eq!(sheets[1]["digest"], Value::Null);
        assert_eq!(
            out["protection"]["workbook"]["element"],
            json!(false),
            "ODF 没有工作簿那一层"
        );
    }

    /// `.xls` 那一族的锁按表记：两份件唯一的差别是锁在第一张还是第二张表，
    /// 而那三条记录跟着挪窝。期望值来自 `lyco_legacy.py` 对同两份件的读取；
    /// 「这位为真就是锁上了」另有一证 —— LibreOffice 自己 import 回 .ods 时
    /// 只在同一张表上写 `table:protected="true"`
    #[test]
    fn legacy_xls_attributes_its_lock_to_the_sheet_that_carries_it() {
        let hand = run("locked-sheet.xls");
        let locks = hand["protection"]["sheets"].as_array().expect("是数组");
        assert_eq!(locks.len(), 3, "{hand}");
        assert_eq!(locks[0]["name"], json!("预算表"));
        assert_eq!(locks[0]["protected"], json!(true));
        assert_eq!(locks[0]["password"], json!(true));
        assert_eq!(locks[0]["password_hash"], json!("6e4e"));
        assert_eq!(
            locks[0]["records"],
            json!({"0x0012": 1, "0x0013": 28238, "0x00dd": 1}),
            "原值也要留着：0x00DD 这一条没有第二个读者认得，不替它编开关名"
        );
        assert_eq!(locks[1]["protected"], json!(false), "锁不在第二张");
        assert_eq!(locks[1]["records"], json!({}));
        assert_eq!(locks[2]["records"], json!({}));

        let second = run("locked-second.xls");
        let moved = second["protection"]["sheets"].as_array().expect("是数组");
        assert_eq!(moved[0]["protected"], json!(false), "{second}");
        assert_eq!(moved[1]["name"], json!("说明"));
        assert_eq!(moved[1]["protected"], json!(true));
        assert_eq!(moved[1]["password_hash"], json!("6e4e"));
        assert_eq!(moved[2]["records"], json!({}));

        let plain = run("book.xls");
        assert!(plain["protection"]["sheets"]
            .as_array()
            .expect("是数组")
            .iter()
            .all(|one| one["protected"] == json!(false) && one["records"] == json!({})));
        assert!(plain["notes"]
            .as_array()
            .expect("说明")
            // 这里查的是「没锁就别谈锁」，所以要用只有那条锁说明才会用的词。
            // 先前查的是「子流」，而批注那条说明（每次都交）也说注住在表子流末尾 ——
            // 一个偶然同词就把这条守卫弄红了，换一个专有的
            .iter()
            .all(|one| !one.as_str().unwrap_or_default().contains("表锁")));
    }

    /// `.xls` 的数字格式那一跳：格子的 ixfe 是 XF 记录的**出现序号**，XF 自报的格式号
    /// 在正文偏移 2，自定义号（>=164）的串在 FORMAT 记录里、内置号查那张内置表。
    /// 换算出来的日期与 LibreOffice 自己把这份 .xls 读回 .ods 交出的 date-value 逐格一致
    #[test]
    fn legacy_xls_takes_the_same_style_hop_as_xlsx() {
        let out = run("formats.xls");
        assert_eq!(
            out["workbook"]["date1904"],
            json!(false),
            "DATEMODE 说的是 1900"
        );
        assert_eq!(out["workbook"]["xfs"].as_array().map(Vec::len), Some(28));
        assert_eq!(out["workbook"]["formats"]["165"], json!("yyyy\\-mm\\-dd"));
        let one = |sheet: &str, reference: &str| -> Value {
            out["cells"]
                .as_array()
                .and_then(|list| {
                    list.into_iter()
                        .find(|had| had["sheet"] == json!(sheet) && had["ref"] == json!(reference))
                        .cloned()
                })
                .unwrap_or(Value::Null)
        };
        let day = one("格式", "C1");
        assert_eq!(day["num_fmt"], json!(165));
        assert_eq!(day["format_kind"], json!("date"));
        assert_eq!(
            day["as_date"],
            json!("2013-12-23"),
            "格式串说它是日期，序列数就换算"
        );
        let stamp = one("格式", "C2");
        assert_eq!(stamp["format_kind"], json!("datetime"));
        assert_eq!(stamp["as_date"], json!("2013-12-23T15:15:00"));
        let part = one("格式", "C3");
        assert_eq!(part["format_kind"], json!("percent"));
        assert!(part.get("as_date").is_none(), "百分比不是日期");
        // 币符在 BIFF 里写成转义的字面量（\\¥），与 ODF 那种「字面量写在格式里」同一种存法：
        // 照字面判成数，不替它认成货币
        assert_eq!(one("格式", "C4")["format_kind"], json!("number"));
        assert_eq!(
            one("格式", "C5")["as_date"],
            json!("2013-12-23"),
            "汉字字面量不挡判定"
        );
        assert_eq!(
            one("格式", "C6")["num_fmt"],
            json!(164),
            "General 在这份件里是自定义号，不是内置 0"
        );
        assert_eq!(
            one("格式", "C7")["format_kind"],
            json!("text"),
            "文本格不拿格式串猜它是什么"
        );
        assert_eq!(one("另一张", "A1")["as_date"], json!("2026-09-23"));

        // 同一判断也管 CSV：日期格出去的是 ISO，不是序列数
        let grid = run_csv("formats.xls", "格式");
        let shown = grid["csv"]["text"].as_str().unwrap_or_default();
        assert!(shown.contains("2013-12-23"), "{shown}");
        assert!(!shown.contains("41631"), "换了就不该再看见序列数：{shown}");
    }

    /// 开 `--csv` 的那条路：`sheet` 是给 `--sheet` 的原样字符串（空=第一张）
    fn run_csv(name: &str, sheet: &str) -> Value {
        let app = OfficeSheet {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office")
                .join(name),
            limit: 200,
            max_bytes: 1 << 26,
            csv: true,
            sheet: sheet.to_string(),
        };
        let (tx, _rx) = mpsc::channel();
        run_office_sheet(&app, &Context::new_test(tx)).expect("office-sheet 应成功")
    }

    /// openpyxl 那份工作簿：三张表（含一张隐藏）、各自的范围与格子数
    /// （期望值来自 `office_reader.py` 的 xlsx_facts）
    #[test]
    fn lists_every_sheet_including_the_hidden_one() {
        let out = run("book.xlsx");
        assert_eq!(out["kind"], "spreadsheetml");
        assert_eq!(out["workbook"]["sheets"], 3, "{out}");
        assert_eq!(out["workbook"]["hidden_sheets"], 1);
        let sheets = out["sheets"].as_array().expect("是数组");
        assert_eq!(sheets[0]["name"], "预算表");
        assert_eq!(sheets[0]["dimension"], "A1:B5", "{}", sheets[0]);
        assert_eq!(sheets[0]["cells"], 9);
        assert_eq!(sheets[0]["formulas"], 1);
        assert_eq!(sheets[0]["merged"], 1);
        assert_eq!(sheets[0]["state"], "visible");
        assert_eq!(sheets[2]["name"], "草稿");
        assert_eq!(sheets[2]["state"], "hidden");
        // openpyxl 不用共享字符串表，也不写缓存结果：两件事都要报出来而不是被含糊过去
        assert_eq!(out["workbook"]["shared_strings"], 0);
        assert_eq!(
            sheets[0]["formula_cells_with_cached_value"], 0,
            "文件里没有缓存值"
        );
        assert_eq!(out["workbook"]["has_calc_chain"], json!(false));
        assert_eq!(out["workbook"]["tables"], 1);
        // 没开 --csv 时这个键是 null，而不是一份空 CSV 装作有
        assert!(
            matches!(out.get("csv"), Some(one) if one.is_null()),
            "{out}"
        );
        assert_eq!(out["defined_names"][0]["text"], "'预算表'!$B$4", "{out}");
        assert!(
            out["notes"].as_array().expect("有 notes").is_empty(),
            "{out}"
        );
    }

    /// 内联字符串要还原成文字，不能只报一个 SST 索引
    #[test]
    fn inline_strings_become_their_text() {
        let out = run("book.xlsx");
        let cells = out["sheets"][0]["cell_list"].as_array().expect("是数组");
        let first = &cells[0];
        assert_eq!(first["ref"], "A1");
        assert_eq!(first["value"], "科目", "{first}");
        assert_eq!(
            cells[3]["value"],
            json!(124000.0),
            "数字格子要报成数：{}",
            cells[3]
        );
        assert_eq!(
            cells[7]["formula"], "SUM(B2:B3)",
            "公式格子报公式：{}",
            cells[7]
        );
    }

    /// 遗留 .xls 走 BIFF8：表名、可见性、字符串表与格子都读得出来，
    /// 并按表归位（BOUNDSHEET 自报的子流起点）
    #[test]
    fn reads_a_legacy_workbook() {
        let out = run("book.xls");
        assert_eq!(out["kind"], "biff8");
        assert_eq!(out["workbook"]["sheets"], 3, "{out}");
        assert_eq!(out["workbook"]["hidden_sheets"], 1);
        assert_eq!(out["workbook"]["shared_strings"], 8);
        assert_eq!(out["workbook"]["totals"]["cells"], 11, "{out}");
        assert_eq!(out["workbook"]["totals"]["formulas"], 1);
        assert_eq!(out["sheets"][0]["cells"], 9, "{out}");
        assert_eq!(out["sheets"][1]["cells"], 1);
        assert_eq!(out["sheets"][2]["cells"], 1, "隐藏表也有格子");
        let named: Vec<String> = out["cells"]
            .as_array()
            .expect("是数组")
            .iter()
            .filter_map(|one| one["sheet"].as_str().map(|one| one.to_string()))
            .collect();
        assert_eq!(
            named.iter().filter(|one| *one == "预算表").count(),
            9,
            "{named:?}"
        );
        assert!(named.iter().any(|one| one == "草稿"), "{named:?}");
    }

    /// 数字格式：`s=` 是 `cellXfs` 的下标。日期格要认出来并换算，而 `12/23/2013`
    /// 那种长得像日期的**文本**不许被猜成日期（期望值来自 `lyco_formats.py`）
    #[test]
    fn a_cell_number_is_not_a_date_until_the_style_says_so() {
        let out = run("formats.xlsx");
        assert_eq!(out["kind"], "spreadsheetml");
        assert_eq!(out["workbook"]["date1904"], json!(false), "{out}");
        assert_eq!(out["workbook"]["totals"]["dates"], 4, "{out}");
        let cells = out["sheets"][0]["cell_list"].as_array().expect("是数组");
        let find = |want: &str| {
            cells
                .iter()
                .find(|one| one["ref"] == want)
                .cloned()
                .unwrap_or(Value::Null)
        };
        assert_eq!(find("C1")["format_kind"], "date", "{out}");
        assert_eq!(find("C1")["as_date"], "2013-12-23", "{}", find("C1"));
        assert_eq!(find("C2")["as_date"], "2013-12-23T15:15:00");
        assert_eq!(find("C3")["format_kind"], "percent");
        assert_eq!(find("C4")["format_kind"], "currency");
        assert_eq!(
            find("C5")["format"],
            "yyyy\"年\"m\"月\"d\"日\"",
            "{}",
            find("C5")
        );
        assert_eq!(find("C6")["format_kind"], "general");
        let trap = find("C7");
        assert_eq!(
            trap["format_kind"], "text",
            "长得像日期的文本仍是文本：{trap}"
        );
        assert!(trap.get("as_date").is_none(), "{trap}");
        assert_eq!(find("A1")["style"], 0, "没写 s 就是 0 号样式");
        assert_eq!(out["sheets"][1]["date_cells"], 1, "{out}");
    }

    /// ODS 走的是另一套账：格子内不写序列数，位置靠重复计数累加，
    /// 隐藏表的可见性在它引的自动样式里（期望值来自 `ods_facts()`）
    #[test]
    fn an_opendocument_spreadsheet_is_read_on_its_own_terms() {
        let out = run("book.ods");
        assert_eq!(out["kind"], "opendocument-spreadsheet");
        assert_eq!(out["workbook"]["sheets"], 3, "{out}");
        assert_eq!(out["workbook"]["hidden_sheets"], 1);
        assert_eq!(
            out["workbook"]["cell_total"], 11,
            "生产者自己写的 cell-count"
        );
        assert_eq!(out["workbook"]["totals"]["formulas"], 1);
        assert_eq!(out["workbook"]["totals"]["covered"], 1);
        let states: Vec<&str> = out["sheets"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one["state"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(states, ["visible", "visible", "hidden"], "{out}");
        let first = &out["sheets"][0];
        assert_eq!(first["columns"], 2, "行尾那 16382 个空格不许算列：{first}");
        assert_eq!(first["value_types"]["string"], 6, "{first}");
        assert_eq!(first["value_types"]["float"], 3, "{first}");
        let listed = first["cell_list"].as_array().expect("是数组");
        let merged = listed
            .iter()
            .find(|one| one["ref"] == "A5")
            .expect("A5 那一格跨两列");
        assert_eq!(merged["columns_spanned"], 2, "{merged}");
        assert_eq!(merged["text"], "口径：含税");

        let formats = run("formats.ods");
        assert_eq!(formats["workbook"]["totals"]["dates"], 4, "{formats}");
        let kinds: Vec<&str> = formats["sheets"][0]["cell_list"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| one["kind"].as_str().unwrap_or_default())
            .collect();
        assert_eq!(
            kinds,
            [
                "string",
                "string",
                "date",
                "date",
                "percentage",
                "float",
                "date",
                "float",
                "string",
                "boolean"
            ],
            "{formats}"
        );
    }

    /// 一个格子的字可以分成几段（富文本），而首尾的空格是文件写的字。
    /// 两家生产者把同一批字放在两处：openpyxl 只写**行内串**（一条 sharedStrings 都没有），
    /// LibreOffice 重写时全搬进字符串表，并在表上自报 `count="8"` 配 `uniqueCount="7"` ——
    /// 那条「甲」被两个格子用了，所以两个数都是对的。`.ods` 是第三种写法：空格与制表符
    /// 不写成字面而写成 `text:s` / `text:tab` 记号。期望值全部来自 `office_reader.py`，
    /// 而 CSV 那一条与 LibreOffice 自己的导出逐字相同（`rich*.csv` 量过）
    #[test]
    fn a_cells_text_can_be_written_in_several_runs() {
        let openpyxl = run("rich.xlsx");
        let rewritten = run("rich-lo.xlsx");
        assert_eq!(
            openpyxl["workbook"]["shared_strings"], 0,
            "一家整个没写这张表"
        );
        assert_eq!(openpyxl["workbook"]["sst_count_written"], Value::Null);
        assert_eq!(openpyxl["workbook"]["sst_unique_matches"], json!(false));
        assert_eq!(rewritten["workbook"]["shared_strings"], 7);
        assert_eq!(rewritten["workbook"]["sst_count_written"], "8");
        assert_eq!(rewritten["workbook"]["sst_unique_written"], "7");
        assert_eq!(rewritten["workbook"]["sst_unique_matches"], json!(true));
        assert_eq!(rewritten["workbook"]["sst_with_runs"], 3);
        assert_eq!(openpyxl["workbook"]["totals"]["cells_with_runs"], 2);
        assert_eq!(rewritten["workbook"]["totals"]["cells_with_runs"], 3);
        assert_eq!(
            openpyxl["workbook"]["totals"]["cells_with_preserved_space"], 2,
            "openpyxl 只给真有空格的那两格写 xml:space"
        );
        assert_eq!(
            rewritten["workbook"]["totals"]["cells_with_preserved_space"], 8,
            "LibreOffice 每一格都写"
        );
        let cells = &openpyxl["sheets"][0]["cell_list"];
        assert_eq!(
            cells[3]["value"], "  两头有空格  ",
            "首尾那两个空格不许被吃掉"
        );
        assert_eq!(cells[3]["space_preserved"], json!(true));
        assert_eq!(cells[7]["value"], "\ttab 开头");
        assert_eq!(cells[4]["value"], "第一行\n第二行");
        assert_eq!(cells[2]["rich_string"], json!(true));
        assert_eq!(cells[2]["run_total"], 2);
        assert_eq!(cells[2]["value"], "重要普通", "分段不改整串的字");
        assert_eq!(
            cells[2]["runs"][0]["format"],
            json!([
                {"element": "rFont", "attrs": {"val": "宋体"}},
                {"element": "b", "attrs": {"val": "1"}},
                {"element": "color", "attrs": {"rgb": "FFC00000"}},
                {"element": "sz", "attrs": {"val": "11"}},
            ]),
            "格式写在 rPr 的孩子上，一家把粗体写成 1：{}",
            cells[2]["runs"][0]["format"]
        );
        // 同一段字，重写那份把同一个开关写成 true，还多补了 family 与 charset
        assert_eq!(
            rewritten["sheets"][0]["cell_list"][2]["runs"][0]["format"][0]["attrs"]["val"],
            "true"
        );
        assert_eq!(
            rewritten["sheets"][0]["cell_list"][2]["runs"][0]["element"],
            "r"
        );
        // 「这一段没写格式」与「那段格式是空的」不是一回事：openpyxl 第一段整个没有 rPr
        assert_eq!(cells[6]["runs"][0]["props_written"], json!(false));
        assert_eq!(cells[6]["runs"][0]["format"], Value::Null);
        assert_eq!(cells[6]["runs"][0]["text"], "整段一个格式");
        assert_eq!(cells[6]["runs"][1]["props_written"], json!(true));
        assert_eq!(
            rewritten["sheets"][0]["cell_list"][7]["run_total"], 2,
            "LO 按字体 fallback 把那一格切成两段"
        );
        assert_eq!(
            rewritten["sheets"][0]["cell_list"][7]["value"],
            "\ttab 开头"
        );
        assert_eq!(
            rewritten["sheets"][0]["cell_list"][0]["rich_string"],
            json!(false)
        );
        assert_eq!(
            run_csv("rich.xlsx", "")["csv"]["text"],
            "甲,整格加粗（格式在格子上不在串里）\n重要普通,\n  两头有空格  ,\n\
             \"第一行\n第二行\",\n甲,\n整段一个格式斜体那截,\n\ttab 开头,\n",
            "铺平之后空格还在原处"
        );
        let ods = run("rich.ods");
        assert_eq!(ods["sheets"][0]["cells"], 8, "记号展开之后这些格才算有内容");
        let list = ods["sheets"][0]["cell_list"].as_array().expect("是数组");
        let at = |want: &str| -> Value {
            list.iter()
                .find(|one| one["ref"].as_str() == Some(want))
                .cloned()
                .unwrap_or(Value::Null)
        };
        assert_eq!(
            at("A3")["text"],
            "  两头有空格  ",
            ".ods 把空格写成 text:s 记号"
        );
        assert_eq!(at("A3")["specials"], 2, "两个记号，四个空格");
        assert_eq!(at("A7")["text"], "\ttab 开头");
        assert_eq!(at("A7")["specials"], 1);
        assert_eq!(at("A2")["text"], "重要普通");
        assert_eq!(at("A2")["spans"], 2, "两段各点一份字符样式");
        assert_eq!(at("A6")["text"], "整段一个格式斜体那截");
        assert_eq!(at("A6")["spans"], 1, "只有一段点了样式");
        assert_eq!(at("A1")["spans"], 0);
        assert_eq!(at("A1")["specials"], 0);
    }

    /// 一个格子的「长相」在 `cellXfs` 之外的三张表里：`s=` → 一条 `xf` →
    /// `fontId` / `fillId` / `borderId` 各查一张表。两家生产者补的东西差很多 ——
    /// openpyxl 只写它觉得要写的（占位那格是**空的** `<patternFill/>`、`A3` 连 `s` 都不写、
    /// 没有 `alignment` 元素），LibreOffice 重写同一份时 `s` 每格都写、`patternType="none"`
    /// 说满、九份字体全表出来，还给每一格补一份写着 `wrapText="false"` 的 `alignment`。
    /// 「文件没说」（null）与「文件说了关」（false）因此是两件事。期望值来自
    /// `office_reader.py` 的 `xlsx_style_tables()` / `style_appearance()`
    #[test]
    fn what_a_cell_looks_like_is_one_hop_away() {
        let openpyxl = run("styled.xlsx");
        let rewritten = run("styled-lo.xlsx");
        assert_eq!(
            openpyxl["workbook"]["styles"]["fonts"],
            json!({"written": "5", "found": 5, "whole": true})
        );
        assert_eq!(
            openpyxl["workbook"]["styles"]["cell_style_xfs"],
            json!({"written": "1", "found": 1, "whole": true})
        );
        assert_eq!(
            rewritten["workbook"]["styles"]["fonts"]["found"], 9,
            "同一批格子，重写那份多补出四份字体"
        );
        assert_eq!(
            rewritten["workbook"]["styles"]["cell_style_xfs"]["found"],
            20
        );
        let cells = &openpyxl["sheets"][0]["cell_list"];
        assert_eq!(
            cells[0]["style_font"],
            json!({
                "attrs": {},
                "parts": [
                    {"element": "name", "attrs": {"val": "微软雅黑"}, "parts": []},
                    {"element": "b", "attrs": {"val": "1"}, "parts": []},
                    {"element": "color", "attrs": {"rgb": "FFC00000"}, "parts": []},
                    {"element": "sz", "attrs": {"val": "12"}, "parts": []},
                ],
            }),
            "格式住在 font 行的孩子上：{}",
            cells[0]["style_font"]
        );
        // 颜色还要再下一层：`fill > patternFill > fgColor`，只走一层的话
        // 「这格什么底色」在文件里明明写着而交不出来
        assert_eq!(
            cells[7]["style_fill"]["parts"][0]["attrs"]["patternType"],
            "lightGrid"
        );
        assert_eq!(
            cells[7]["style_fill"]["parts"][0]["parts"][0],
            json!({"element": "fgColor", "attrs": {"rgb": "FF00B050"}, "parts": []}),
            "点状网格那一格的前景色：{}",
            cells[7]["style_fill"]
        );
        assert_eq!(
            rewritten["sheets"][0]["cell_list"][7]["style_fill"]["parts"][0]["parts"][0]["attrs"]
                ["rgb"],
            "FF90DDB3",
            "同一条底色被 LibreOffice 换成实心底与另一个颜色串"
        );
        assert_eq!(cells[0]["style_bold"], json!(true));
        assert_eq!(cells[0]["style_font_id"], "1");
        assert_eq!(
            cells[1]["style_filled"],
            json!(true),
            "实心黄底那一格自己说了 solid：{}",
            cells[1]["style_fill"]
        );
        assert_eq!(
            cells[1]["style_fill"]["parts"][0]["attrs"]["patternType"],
            "solid"
        );
        // 占位那一条：openpyxl 写的是**空的** `<patternFill/>`，LibreOffice 写明 none
        assert_eq!(
            cells[9]["style_fill"]["parts"][0]["attrs"],
            json!({}),
            "空的 patternFill 与 patternType=\"none\" 是两种写法：{}",
            cells[9]["style_fill"]
        );
        assert_eq!(cells[9]["style_filled"], json!(false));
        // 没写 `alignment` 与写了 `wrapText="false"` 也不是一回事
        assert_eq!(cells[0]["style_alignment"], Value::Null);
        assert_eq!(cells[0]["style_wrapped"], Value::Null);
        assert_eq!(
            rewritten["sheets"][0]["cell_list"][0]["style_wrapped"],
            json!(false)
        );
        assert_eq!(cells[4]["style_wrapped"], json!(true));
        assert_eq!(cells[4]["style_alignment"]["horizontal"], "right");
        // 「这一格没写 s」与「这一格写了 s=\"0\"」：一家不写、另一家每格都写
        assert_eq!(cells[9]["style_written"], json!(false));
        assert_eq!(
            rewritten["sheets"][0]["cell_list"][9]["style_written"],
            json!(true)
        );
        assert_eq!(
            cells[9]["style_found"],
            json!(true),
            "没写按 0 查，仍查得到那一条"
        );
        assert_eq!(
            cells[9]["style_bold"],
            Value::Null,
            "第 0 条字体没有 b 孩子：那是没说，不是关"
        );
        for (one, want) in [
            (&openpyxl, (1u64, 2u64, 1u64, 1u64)),
            (&rewritten, (1u64, 2u64, 1u64, 0u64)),
        ] {
            assert_eq!(one["sheets"][0]["cells_bold"], want.0, "{one}");
            assert_eq!(one["sheets"][0]["cells_filled"], want.1);
            assert_eq!(one["sheets"][0]["cells_wrapped"], want.2);
            assert_eq!(one["sheets"][0]["cells_without_style_written"], want.3);
        }
    }

    /// `--csv`：一份铺平的网格（期望文本逐字来自 `office_reader.py` 的 csv_facts）
    #[test]
    fn csv_renders_the_grid_of_a_sheet() {
        let out = run_csv("book.ods", "");
        let csv = &out["csv"];
        assert_eq!(csv["sheet"], "预算表");
        assert_eq!(csv["index"], 0);
        assert_eq!(csv["rows"], 5, "{csv}");
        assert_eq!(csv["columns"], 2);
        assert_eq!(csv["cells_skipped"], 0);
        assert_eq!(csv["line_end"], "LF");
        assert_eq!(
            csv["text"], "科目,金额\n服务器,124000\n网络,18000\n合计,142000\n口径：含税,\n",
            "{csv}"
        );
    }

    /// 公式格没有缓存值时 CSV 里那一格是空的：这不是漏，是文件本来就没给数
    #[test]
    fn a_formula_without_a_cached_result_renders_empty() {
        let out = run_csv("book.xlsx", "");
        assert_eq!(
            out["csv"]["text"],
            "科目,金额\n服务器,124000\n网络,18000\n合计,\n口径：含税,\n"
        );
        assert_eq!(out["csv"]["rows"], 5);
        assert_eq!(out["sheets"][0]["formula_cells_with_cached_value"], 0);
    }

    /// 「结果不是数」那几种：除零、拼出来的字、布尔 —— 一律按文件写着什么交什么
    /// （期望值来自 `office_reader.py`；`errors-lo.xlsx` 那一份另与 LibreOffice 自己的
    /// CSV 导出逐格对过，只有 `#VALUE!` 那一格它不交缓存值而交重算后的本地化名）
    #[test]
    fn a_result_that_is_not_a_number_goes_out_as_written() {
        let out = run("errors-lo.xlsx");
        let sheet = &out["sheets"][0];
        assert_eq!(sheet["cells"], 9);
        assert_eq!(sheet["error_cells"], 3, "除零、NA()、VLOOKUP 指不到");
        assert_eq!(sheet["string_result_cells"], 1, "拼出来的那一句字");
        assert_eq!(
            sheet["formulas"], 7,
            "连那个布尔常量也被写成了 TRUE() 一条公式"
        );
        assert_eq!(sheet["formula_cells_with_cached_value"], 7);
        assert_eq!(sheet["cells_with_written_type"], 9, "这一家每一格都写了 t");
        assert_eq!(out["workbook"]["totals"]["error_cells"], 3);
        let cells = sheet["cell_list"].as_array().expect("是数组");
        assert_eq!(cells[1]["ref"], "B1");
        assert_eq!(cells[1]["kind"], "e");
        assert_eq!(cells[1]["kind_written"], json!(true));
        assert_eq!(
            cells[1]["value"], "#DIV/0!",
            "文件自己就写着这一串，一字不加"
        );
        assert_eq!(cells[1]["formula"], "1/0");
        assert_eq!(cells[2]["kind"], "b");
        assert_eq!(
            cells[2]["value"], "TRUE",
            "布尔那一格文件写的是 1，显示的是 TRUE"
        );
        assert_eq!(cells[4]["kind"], "str");
        assert_eq!(cells[4]["value"], "甲乙");
        assert_eq!(cells[5]["value"], "#N/A");
        assert_eq!(cells[6]["value"], "#VALUE!");
        assert_eq!(
            run_csv("errors-lo.xlsx", "")["csv"]["text"],
            "7,#DIV/0!,,TRUE\n5,甲乙,,\n,#N/A,,\n,#VALUE!,,\n,14,10,\n"
        );

        // 生产者没重算就没有错误格：openpyxl 那份只抄公式，`t` 一个都不写
        let quiet = run("errors.xlsx");
        let sheet = &quiet["sheets"][0];
        assert_eq!(sheet["error_cells"], 0, "没缓存值就没判出错误");
        assert_eq!(sheet["string_result_cells"], 0);
        assert_eq!(sheet["formula_cells_with_cached_value"], 0);
        assert_eq!(
            sheet["cells_with_written_type"], 3,
            "只有 A1 / A2 / D1 写了 t"
        );
        let cells = sheet["cell_list"].as_array().expect("是数组");
        assert_eq!(cells[1]["ref"], "B1");
        assert_eq!(cells[1]["kind"], "n", "t 没写时按规范默认 n");
        assert_eq!(
            cells[1]["kind_written"],
            json!(false),
            "那是默认不是文件说的话"
        );

        // ODF 那一种摆法：错误格的 office:value-type 写的是 string，
        // 那串显示值只在 <text:p> 里；LibreOffice 另写的 calcext:value-type="error" 不跟
        let ods = run("errors.ods");
        let cells = ods["sheets"][0]["cell_list"].as_array().expect("是数组");
        let at = |which: &str| -> Value {
            cells
                .iter()
                .find(|one| one["ref"].as_str() == Some(which))
                .cloned()
                .unwrap_or(Value::Null)
        };
        let b1 = at("B1");
        assert_eq!(
            b1["kind"], "string",
            "文件写的就是 string，不替它改成 error"
        );
        assert_eq!(b1["text"], "#DIV/0!");
        assert_eq!(b1["value"], Value::Null, "office:string-value 是空的");
        assert_eq!(at("B2")["text"], "甲乙", "算成字的那一格是同一种摆法");
        assert_eq!(
            at("B4")["text"],
            "错误:502",
            "同一个错误在 ODF 那一族被本地化成了另一个名字，照文件交：{}",
            at("B4")
        );
    }

    /// 1904 基准：同一批序列数换一套基准就是另一套日子
    /// （期望值来自 `lyco_formats.py` 与 LibreOffice 自己的 CSV 渲染）
    #[test]
    fn a_1904_workbook_counts_days_from_the_epoch_it_stated() {
        for name in ["epoch.xlsx", "epoch-lo.xlsx"] {
            let out = run(name);
            // 两家写这个开关的拼法不同（`"1"` 与 `"true"`），认出来的基准是同一个
            assert_eq!(out["workbook"]["date1904"], json!(true), "{name}");
            let cells = out["sheets"][0]["cell_list"].as_array().expect("是数组");
            assert_eq!(cells[0]["value"].as_f64(), Some(40169.0), "{name}");
            assert_eq!(cells[0]["as_date"], "2013-12-23", "{name}");
            assert_eq!(cells[2]["as_date"], "2020-01-02T03:04:05", "{name}");
            // 序列号 60 那个不存在的 1900-02-29 只属于 1900 基准，这里它是好好的一天
            assert_eq!(cells[3]["value"].as_f64(), Some(60.0), "{name}");
            assert_eq!(cells[3]["as_date"], "1904-03-01", "{name}");
            assert_eq!(
                cells[4]["as_date"],
                Value::Null,
                "看着像日期的那一格本来就是字：{name}"
            );
            assert_eq!(
                run_csv(name, "")["csv"]["text"],
                "2013-12-23,1,2020-01-02T03:04:05,1904-03-01,12/23/2013\n",
                "{name}"
            );
        }
        let openpyxl = run("epoch.xlsx");
        let cells = openpyxl["sheets"][0]["cell_list"]
            .as_array()
            .expect("是数组");
        assert_eq!(
            cells[4]["kind"], "inlineStr",
            "openpyxl 把那条字写成 inline 的"
        );
        assert_eq!(cells[4]["value"], "12/23/2013");
        let rewritten = run("epoch-lo.xlsx");
        let cells = rewritten["sheets"][0]["cell_list"]
            .as_array()
            .expect("是数组");
        assert_eq!(cells[4]["kind"], "s", "LibreOffice 重写时换成共享字符串");
        assert_eq!(cells[4]["value"], "12/23/2013", "换存法不换字");
    }

    /// `--sheet` 认表名也认命令报出的序号，认不出来就把候选说清楚
    #[test]
    fn csv_picks_a_sheet_by_name_or_by_index() {
        let by_name = run_csv("book.ods", "草稿");
        assert_eq!(by_name["csv"]["sheet"], "草稿");
        assert_eq!(by_name["csv"]["index"], 2);
        assert_eq!(by_name["csv"]["text"], "隐藏的草稿表\n");
        let by_index = run_csv("book.ods", "1");
        assert_eq!(by_index["csv"]["sheet"], "说明", "序号从 0 起");
        let missing = run_csv("book.ods", "没这张");
        assert!(
            missing["csv"]["error"]
                .as_str()
                .unwrap_or_default()
                .contains("预算表"),
            "{missing}"
        );
        assert!(
            run_csv("book.ods", "9")["csv"]["error"].is_string(),
            "序号越界也要走 error 那条话，不能默默给第一张"
        );
    }

    /// 日期格进 CSV 给 ISO 读法，百分数与货币给值不给 `12.5%` 那一份
    /// （与 `--csv` 之外那套格式账同源，两边不许各编一个数）
    #[test]
    fn csv_gives_values_not_display_strings() {
        let out = run_csv("formats.xlsx", "");
        assert_eq!(
            out["csv"]["text"],
            "标签,日期,2013-12-23\n,,2013-12-23T15:15:00\n,,0.125\n,,124000\n\
             ,,2013-12-23\n,,1234.5\n,,12/23/2013\n,,TRUE\n",
            "{out}"
        );
        assert_eq!(out["csv"]["rows"], 8);
        assert_eq!(out["csv"]["columns"], 3);
    }

    /// 同一份数据在 xlsx 与 ods 两边拼出来的 CSV 必须逐字相同 —— 这两套账
    /// 一个是序列数 + 格式码、一个是显式 value-type，能对上才说明两边都没猜
    #[test]
    fn the_same_workbook_renders_the_same_csv_in_both_formats() {
        for sheet in ["", "另一张"] {
            let xlsx = run_csv("formats.xlsx", sheet);
            let ods = run_csv("formats.ods", sheet);
            assert_eq!(xlsx["csv"]["text"], ods["csv"]["text"], "sheet={sheet}");
        }
        for sheet in ["", "说明", "草稿"] {
            let xlsx = run_csv("book.xlsx", sheet);
            let ods = run_csv("book.ods", sheet);
            if sheet.is_empty() {
                // 只有第一张差一格：那边公式没缓存值，这边 LibreOffice 写了 142000
                assert_ne!(xlsx["csv"]["text"], ods["csv"]["text"]);
            } else {
                assert_eq!(xlsx["csv"]["text"], ods["csv"]["text"], "sheet={sheet}");
            }
        }
    }

    /// 表格里的批注**不在** `sheetN.xml` 里，要靠这张表自己的关系表跳到批注部件。
    /// 两个生产者把那个部件放在两个地方（openpyxl `xl/comments/comment1.xml`、
    /// LibreOffice `xl/comments1.xml`），关系 Target 一个绝对一个相对，条目顺序还不一样
    /// —— 三份件（两份 xlsx 加一份 .ods）必须读出同一批 (格子, 作者, 字)。
    /// 期望值来自 `office_reader.py` 的 `xlsx_comments()` 与 `ods_facts()`
    #[test]
    fn spreadsheet_comments_are_reached_through_the_sheet_relationships() {
        let expect: Vec<(String, String, String)> = vec![
            (
                "A3".to_string(),
                "李四".to_string(),
                "第二张单已确认".to_string(),
            ),
            (
                "B2".to_string(),
                "张三".to_string(),
                "这里要补上不含税口径".to_string(),
            ),
            (
                "B3".to_string(),
                "李四".to_string(),
                "同一个作者再来一条".to_string(),
            ),
        ];
        for name in ["cell-notes.xlsx", "cell-notes-lo.xlsx", "cell-notes.ods"] {
            let out = run(name);
            let sheet = &out["sheets"][0];
            assert_eq!(sheet["comments"], 3, "{name}：{sheet}");
            let mut got: Vec<(String, String, String)> = sheet["comment_list"]
                .as_array()
                .expect("是数组")
                .iter()
                .map(|one| {
                    (
                        one["ref"].as_str().unwrap_or_default().to_string(),
                        one["author"].as_str().unwrap_or_default().to_string(),
                        one["text"].as_str().unwrap_or_default().to_string(),
                    )
                })
                .collect();
            got.sort();
            assert_eq!(got, expect.clone(), "{name}");
            assert_eq!(out["workbook"]["totals"]["comments"], 3, "{name} 的总账");
        }
        // ODF 那份尤其容易混：注就坐在格子里面，一锅端取字就会把它当这一格的内容
        let ods = run("cell-notes.ods");
        let noted = ods["sheets"][0]["cell_list"]
            .as_array()
            .expect("是数组")
            .iter()
            .find(|one| one["ref"] == json!("B2"))
            .expect("有 B2 这一格");
        assert_eq!(noted["text"], json!("124000"), "{noted}");
        // 两个生产者都没往批注里写时间：那就交回 null，不替它编一个
        let dated = run("cell-notes.xlsx")["sheets"][0]["comment_list"]
            .as_array()
            .expect("是数组")
            .clone();
        assert!(dated.iter().all(|one| one["date"].is_null()), "{dated:?}");
        // 反面对照：没有批注的那份报 0，而不是没有这个键
        assert_eq!(run("book.xlsx")["workbook"]["totals"]["comments"], 0);
    }

    /// 第四种存法：`.xls` 的注既不在另一个部件里，也不在格子里面，而在**同一条流**的
    /// 两类记录上 —— 一条给字（正文偏移 10 是它自报的字数，紧跟的第一条 CONTINUE 首字节
    /// 说编码），一条给「哪个格子 + 谁写的」（住在这张表子流的末尾）。两份列表按出现顺序配。
    /// 这份件是一次只改一个变量造出来的：作者名有 ASCII 的也有中文的（那位编码旗标与 BIFF8
    /// 的 fCompressed 惯例相反）、字数有 2 也有 3、注的字带换行、格子拉到 AA100 让列名进
    /// 两位数、注还分到两张表上。期望值来自 `lyco_legacy.py` 的 `biff_workbook()`，
    /// 而 (表, 格子, 字) 这一列另与 LibreOffice 自己把这份 .xls 转回 .xlsx 后读到的对得上
    #[test]
    fn xls_comments_are_paired_out_of_two_record_kinds() {
        let out = run("cell-notes-many.xls");
        assert_eq!(out["workbook"]["totals"]["comments"], 4, "{out}");
        let expect: Vec<Value> = vec![
            json!({"sheet": "预算表", "ref": "A1", "author": "AB", "text": "one"}),
            json!({
                "sheet": "预算表",
                "ref": "C5",
                "author": "张三",
                "text": "第一行\n第二行"
            }),
            json!({
                "sheet": "预算表",
                "ref": "AA100",
                "author": "欧阳锋",
                "text": "这里要补上不含税口径"
            }),
            json!({
                "sheet": "第二张",
                "ref": "B2",
                "author": "李四",
                "text": "第二张单已确认"
            }),
        ];
        let mut got: Vec<Value> = Vec::new();
        for sheet in out["sheets"].as_array().expect("是数组") {
            for one in sheet["comment_list"].as_array().expect("是数组") {
                got.push(json!({
                    "sheet": sheet["name"],
                    "ref": one["ref"],
                    "author": one["author"],
                    "text": one["text"],
                }));
            }
        }
        assert_eq!(got, expect, "{out}");
        for sheet in out["sheets"].as_array().expect("是数组") {
            // 两类记录各自的条数一起交：配的条数只到小的那个，所以对不上时这里看得见
            assert_eq!(sheet["note_text_records"], sheet["comments"], "{sheet}");
            assert_eq!(sheet["note_cell_records"], sheet["comments"], "{sheet}");
            let list = sheet["comment_list"].as_array().expect("是数组");
            assert!(!list.is_empty(), "{sheet}");
            // 两边自报的字数都正好切出来（切不出来会留 whole=false，不装作读全了）
            assert!(
                list.iter().all(|one| one["whole"] == json!(true)),
                "{sheet}"
            );
            // 这一族的三条记录里没有作者时间那个字段，所以交回 null 而不是编一个
            assert!(list.iter().all(|one| one["date"].is_null()), "{sheet}");
        }
        // 不按子流归位就会把第二张表的注挂到第一张上
        assert_eq!(out["sheets"][1]["name"], "第二张", "{out}");
        assert_eq!(out["sheets"][1]["comments"], 1, "{out}");
        assert_eq!(out["sheets"][0]["comments"], 3, "{out}");
        // 反面对照：四份没有注的 .xls 报 0 条，键在、值为零
        assert_eq!(run("book.xls")["workbook"]["totals"]["comments"], 0);
        assert_eq!(run("hidden.xls")["workbook"]["totals"]["comments"], 0);
    }

    /// 隐藏的行与列有四种写法：openpyxl 一列一条 `hidden="1"`、LibreOffice 把连续
    /// 三列并成一条 `min="3" max="5" hidden="true"`、ODF 用
    /// `table:visibility="collapse"` 压在一整条 `number-columns-repeated="3"` 上，
    /// .xls 把它们写成 ROW 记录的一个位与 COLINFO 的一段范围。
    /// 四种存法必须报出同一个数
    #[test]
    fn hidden_rows_and_columns_are_counted_whichever_way_they_are_written() {
        for name in ["hidden.xlsx", "hidden-lo.xlsx", "hidden.ods", "hidden.xls"] {
            let out = run(name);
            let sheet = &out["sheets"][0];
            assert_eq!(sheet["name"], "预算表", "{name}");
            assert_eq!(sheet["hidden_rows"], 2, "{name}：{sheet}");
            assert_eq!(sheet["hidden_cols"], 3, "{name}：{sheet}");
            assert_eq!(sheet["cells"], 13, "藏起来的格子还是格子：{name}");
            // 合计那份也要一致：CI 上第一轮就是「每张表都对、总账是 0」被抓出来的
            assert_eq!(out["workbook"]["totals"]["hidden_rows"], 2, "{name} 的总账");
            assert_eq!(out["workbook"]["totals"]["hidden_cols"], 3, "{name} 的总账");
        }
    }

    /// 隐藏只是「看不见」，不是「没有」：那些字要照样出现在 CSV 里
    #[test]
    fn hidden_columns_still_carry_their_text() {
        for name in ["hidden.xlsx", "hidden-lo.xlsx", "hidden.ods", "hidden.xls"] {
            let text = run_csv(name, "")["csv"]["text"]
                .as_str()
                .unwrap_or_default()
                .to_string();
            assert!(
                text.contains("第一列批注") && text.contains("第二列批注"),
                "{name}：{text}"
            );
        }
    }

    /// ODS 的格式在样式那一跳后面（期望值来自 `office_reader.py` 的 ods_styles）：
    /// 格子只写一个样式名，样式再指数据样式，数据样式才说这是日期、百分数还是布尔
    #[test]
    fn an_ods_cell_reports_the_data_style_it_inherits() {
        let out = run("formats.ods");
        let cells = out["sheets"][0]["cell_list"]
            .as_array()
            .expect("是数组")
            .iter()
            .map(|one| {
                (
                    one["ref"].as_str().unwrap_or_default().to_string(),
                    one.clone(),
                )
            })
            .collect::<std::collections::HashMap<String, Value>>();
        let cell = |want: &str| cells.get(want).cloned().unwrap_or(Value::Null);
        let plain = cell("C1");
        assert_eq!(plain["cell_style"], "ce1", "{plain}");
        assert_eq!(plain["data_style"], "N49");
        assert_eq!(plain["format_kind"], "date");
        assert_eq!(
            plain["format_tokens"],
            json!(["year", "text:-", "month", "text:-", "day"]),
            "元素树逐条抄，不替它拼格式串：{plain}"
        );
        let percent = cell("C3");
        assert_eq!(percent["format_kind"], "percent", "{percent}");
        assert_eq!(percent["decimals"], 1);
        let money = cell("C4");
        assert_eq!(
            money["format_kind"], "number",
            "¥ 是字面量，不是 currency-style"
        );
        assert_eq!(money["currency_symbol"], Value::Null, "{money}");
        let flag = cell("C8");
        assert_eq!(flag["format_kind"], "bool", "{flag}");
        // 没写样式的格子不交一份空对象：那一格本来就没有这一跳
        assert!(cell("C6").get("cell_style").is_none(), "{}", cell("C6"));
        assert!(
            out["workbook"]["styles"]["data_styles"]
                .as_u64()
                .unwrap_or(0)
                > 0
        );
    }

    /// 每张表的打印设置：只报文件自己写的，缺的元素交 null（不是 false），单位按原样交。
    /// 期望值逐字来自 `.scratch/shadow_sheet_print.py` —— python 侧同算法的镜子
    #[test]
    fn print_setup_reports_only_what_each_sheet_written() {
        let hand = run("hidden.xlsx");
        let mine = &hand["sheets"][0]["print_setup"];
        assert_eq!(mine["margin_unit"], "inch", "{mine}");
        assert_eq!(mine["margins"]["left"], "0.75", "{mine}");
        assert_eq!(mine["margins"]["top"], "1");
        assert_eq!(mine["margins"]["header"], "0.5");
        assert_eq!(mine["setup"], Value::Null, "openpyxl 整个不写 pageSetup");
        assert_eq!(
            mine["options"],
            Value::Null,
            "那些开关是「没说」，不是 false"
        );

        let lo = run("hidden-lo.xlsx");
        let theirs = &lo["sheets"][0]["print_setup"];
        assert_eq!(
            theirs["margins"]["header"], "0.511811023622047",
            "边距原样交，换成整数就把两个生产者的差抹平了：{theirs}"
        );
        assert_eq!(theirs["setup"]["paperSize"], "9", "{theirs}");
        assert_eq!(theirs["setup"]["orientation"], "portrait");
        assert_eq!(theirs["setup"]["horizontalDpi"], "300");
        assert_eq!(theirs["setup"]["copies"], "1");
        assert_eq!(theirs["options"]["gridLinesSet"], "true");
        assert_eq!(theirs["options"]["gridLines"], "false");

        // 表多的那份：每张各自一份，不是文档级共用一条
        let many = run("cell-notes-many.xlsx");
        let sheets = many["sheets"].as_array().expect("是数组");
        assert_eq!(sheets.len(), 2, "{many}");
        for one in sheets {
            assert_eq!(one["print_setup"]["margins"]["left"], "0.75", "{one}");
            assert_eq!(one["print_setup"]["setup"], Value::Null, "{one}");
        }

        // 另两族还没读：ODF 的表不点名自己的版式（五份件全量过，那一跳文件里根本没有），
        // .xls 有 SETUP 记录但字段位没量过 —— 键整个不在，而不是空对象
        for name in ["hidden.ods", "hidden.xls", "book.xls"] {
            let out = run(name);
            for one in out["sheets"].as_array().expect("是数组") {
                assert!(
                    one.get("print_setup").is_none(),
                    "{name} 这一族没读，别交出空对象：{one}"
                );
            }
        }
    }

    /// 图那份账：三跳找到图部件，两个生产者的引用写法与缓存值各交各的。
    /// 期望值逐字来自 `.scratch/measure_chart_parts.py` 与 python 侧的镜像 `xlsx_charts()`
    #[test]
    fn charts_are_reached_through_the_drawing_and_report_only_cached_values() {
        let hand = run("chart.xlsx");
        let sheets = hand["sheets"].as_array().expect("是数组");
        assert_eq!(sheets.len(), 2, "{sheets:?}");
        assert_eq!(sheets[0]["charts"], 2, "两张图挂在同一张表上");
        assert_eq!(sheets[1]["charts"], 0, "没有图的那张表报 0，不是缺这个键");
        assert_eq!(hand["workbook"]["totals"]["charts"], 2);

        let first = &sheets[0]["chart_list"][0];
        assert_eq!(first["part"], "xl/charts/chart1.xml", "{first}");
        assert_eq!(first["present"], json!(true));
        assert_eq!(first["cached"], json!(false), "openpyxl 一条 pt 都不缓存");
        assert_eq!(
            first["title"]["via"], "text",
            "字面标题走 `c:rich`：{first}"
        );
        assert_eq!(first["title"]["text"], "逐月收支");
        assert_eq!(first["title"]["ref"], Value::Null);
        let group = &first["groups"][0];
        assert_eq!(group["kind"], "barChart", "{group}");
        assert_eq!(group["written"]["barDir"], "col");
        assert_eq!(group["written"]["grouping"], "clustered");
        assert_eq!(group["written"]["gapWidth"], "150");
        assert_eq!(group["axis_ids"], json!(["10", "100"]));
        assert_eq!(group["series"], 2);
        let ser = &group["series_list"][0];
        assert_eq!(ser["index"], "0", "{ser}");
        assert_eq!(ser["order"], "0", "idx 与 order 各是文件自己写的那一个号");
        assert_eq!(group["series_list"][1]["index"], "1");
        assert_eq!(ser["name"]["via"], "strRef");
        assert_eq!(ser["name"]["text"], Value::Null);
        let line = &sheets[0]["chart_list"][1];
        assert_eq!(line["groups"][0]["kind"], "lineChart", "{line}");
        assert_eq!(line["groups"][0]["series"], 1);
        assert_eq!(line["title"]["text"], "收入折线");
        // 判不住的那一句要说出来：这张表的图没有缓存，图里画的是哪些数只能交引用
        assert!(
            hand["notes"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .any(|one| one.as_str().unwrap_or_default().contains("没有缓存")),
            "{:?}",
            hand["notes"]
        );

        let lo = run("chart-lo.xlsx");
        assert_eq!(lo["workbook"]["totals"]["charts"], 2);
        let one = &lo["sheets"][0]["chart_list"][0];
        assert_eq!(one["cached"], json!(true), "{one}");
        assert_eq!(
            one["title"]["text"], "逐月收支",
            "标题两家都写成字面量：{one}"
        );
        let mine = &one["groups"][0]["series_list"][0];
        assert_eq!(mine["index"], "0");
        assert_eq!(
            mine["name"]["ref"], "数据!$B$1",
            "同一段格子的第二种写法：{mine}"
        );
        assert_eq!(mine["name"]["cache"]["written"], "1");
        assert_eq!(mine["name"]["cache"]["values"], json!(["收入"]));
        assert_eq!(
            mine["cat"]["via"], "strRef",
            "类目在第二家换成了 strRef：{mine}"
        );
        assert_eq!(mine["cat"]["ref"], "数据!$A$2:$A$3");
        assert_eq!(mine["cat"]["cache"]["points"], 2);
        assert_eq!(mine["cat"]["cache"]["values"], json!(["一月", "二月"]));
        assert_eq!(mine["val"]["via"], "numRef");
        assert_eq!(mine["val"]["cache"]["written"], "2");
        assert_eq!(mine["val"]["cache"]["values"], json!([10.0, 25.0]));
        assert_eq!(mine["val"]["cache"]["whole"], json!(true));
        // 轴 id 是生产者自己编的号：两家都是两个，但值本来就不可比，所以只比个数
        assert_eq!(
            one["groups"][0]["axis_ids"]
                .as_array()
                .expect("是数组")
                .len(),
            2
        );
        // 同一批字的两份件：系列数、类目引用与图类型必须一致（引用串除外）
        let count = |one: &Value| {
            one["groups"]
                .as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|had| had["series"].as_u64().unwrap_or(0))
                .collect::<Vec<u64>>()
        };
        assert_eq!(count(first), count(one), "两家喂的是同一批格子");
        let kinds = |list: &Value| {
            list.as_array()
                .unwrap_or(&Vec::new())
                .iter()
                .map(|had| {
                    had["groups"]
                        .as_array()
                        .unwrap_or(&Vec::new())
                        .iter()
                        .map(|one| one["kind"].as_str().unwrap_or_default().to_string())
                        .collect::<Vec<String>>()
                })
                .collect::<Vec<Vec<String>>>()
        };
        assert_eq!(
            kinds(&sheets[0]["chart_list"]),
            kinds(&lo["sheets"][0]["chart_list"]),
            "两张图的类型与先后次序两家一致"
        );

        // 没挂图的表报 0（数过了没有）：xlsx 与 ods 两家都是这个口径
        for name in ["hidden.xlsx", "book.ods", "hidden.ods"] {
            for one in run(name)["sheets"].as_array().expect("是数组") {
                assert_eq!(one["charts"], 0, "{name} 没有图就说没有：{one}");
            }
        }
        // .xls 这一族的图还没读（BIFF 的对象链没有一个读者量过）：键整个不在
        for one in run("hidden.xls")["sheets"].as_array().expect("是数组") {
            assert!(one.get("charts").is_none(), "xls 这一族的图还没读：{one}");
        }
    }

    /// 规则那两份账：条件格式的样式在 dxf 那一跳上，数据验证的开关两种拼法
    /// （期望值逐字来自 `.scratch/mk_rules_fixture.py` 之后 python 侧的 `sheet_rules()`）
    #[test]
    fn rules_report_the_dxf_hop_and_every_switch_as_written() {
        let hand = run("rules.xlsx");
        let sheet = &hand["sheets"][0]["rules"];
        assert_eq!(hand["workbook"]["dxfs"]["written"], "2", "{sheet}");
        assert_eq!(hand["workbook"]["dxfs"]["found"], 2);
        assert_eq!(hand["workbook"]["dxfs"]["whole"], json!(true));
        assert_eq!(hand["workbook"]["totals"]["conditional_rules"], 4);
        assert_eq!(hand["workbook"]["totals"]["validations"], 3);
        assert_eq!(sheet["conditional"].as_array().expect("是数组").len(), 3);

        let block = &sheet["conditional"][0];
        assert_eq!(block["sqref"], "B2:B6", "{block}");
        assert_eq!(block["rules"], 2, "一块范围里两条规则");
        let cell = &block["rule_list"][0];
        assert_eq!(cell["type"], "cellIs", "{cell}");
        assert_eq!(cell["priority"], "1");
        assert_eq!(cell["operator"], "greaterThan");
        assert_eq!(cell["formulas"], json!(["100"]));
        assert_eq!(cell["dxf"]["written"], "0");
        assert_eq!(cell["dxf"]["found"], json!(true), "dxfId 那一跳指得到东西");
        assert_eq!(cell["dxf"]["kinds"], json!(["font/b", "font/color"]));
        assert_eq!(cell["scale"], Value::Null, "cellIs 不写子形状");

        let both = &sheet["conditional"][1];
        assert_eq!(
            both["sqref"], "A2:A6 B2:B4",
            "一条 sqref 里塞两段区间：{both}"
        );
        let scale = &both["rule_list"][0];
        assert_eq!(scale["type"], "colorScale");
        assert_eq!(scale["scale"]["kind"], "colorScale");
        assert_eq!(
            scale["scale"]["colors"],
            json!(["00FFFFFF", "00FFEB84", "00F8696B"]),
            "颜色串按文件写的交，alpha 那两位不归一化"
        );
        assert_eq!(scale["scale"]["cfvo"].as_array().expect("是数组").len(), 3);
        assert_eq!(scale["dxf"]["written"], Value::Null, "色阶不指 dxf");

        let icon = &block["rule_list"][1];
        assert_eq!(icon["type"], "iconSet", "{icon}");
        assert_eq!(icon["scale"]["written"]["iconSet"], "3Arrows");
        assert_eq!(
            icon["scale"]["cfvo"][2],
            json!({"type": "percent", "val": "67"}),
            "cfvo 的 type 与 val 一起交"
        );
        let expr = &sheet["conditional"][2]["rule_list"][0];
        assert_eq!(expr["type"], "expression");
        assert_eq!(
            expr["formulas"],
            json!(["$B2>200"]),
            "那个 > 是实体：{expr}"
        );
        assert_eq!(expr["dxf"]["kinds"], json!(["font/i"]));

        let dv = &sheet["validations"];
        assert_eq!(dv["written"], "3", "容器自报的 count：{dv}");
        assert_eq!(dv["found"], 3);
        assert_eq!(dv["whole"], json!(true));
        let list = &dv["list"][0];
        assert_eq!(list["type"], "list", "{list}");
        assert_eq!(list["sqref"], "D2:D6");
        assert_eq!(
            list["operator"],
            Value::Null,
            "openpyxl 给 list 不写 operator"
        );
        assert_eq!(list["formulas"], json!(["\"红,黄,绿\""]));
        assert_eq!(list["written"]["allowBlank"], "1");
        assert_eq!(list["written"]["showDropDown"], "0");
        assert_eq!(list["written"]["prompt"], "下拉里有三个颜色");
        assert_eq!(dv["list"][2]["formulas"], json!(["=ISNUMBER(B2)"]));

        // LibreOffice 重写同一批规则：优先级是它自己排的、开关换成 true/false、
        // 给 list 补了 operator、给 custom 补了 formula2、把公式开头的 = 去掉，
        // 而且给同一条 dxf 多补了三个元素
        let lo = run("rules-lo.xlsx");
        let theirs = &lo["sheets"][0]["rules"];
        assert_eq!(lo["workbook"]["dxfs"]["found"], 2);
        let other = &theirs["conditional"][0]["rule_list"][0];
        assert_eq!(other["priority"], "2", "优先级各排各的：{other}");
        assert_eq!(
            other["dxf"]["kinds"].as_array().expect("是数组").len(),
            5,
            "多出的 name / family / sz 是它自己补的：{other}"
        );
        assert_eq!(
            other["written"]["aboveAverage"], "0",
            "等于默认的开关它也写：{other}"
        );
        let one_list = &theirs["validations"]["list"][0];
        assert_eq!(one_list["operator"], "equal", "list 也被补了 operator");
        assert_eq!(
            one_list["written"]["allowBlank"], "true",
            "同一条开关的第二种拼法：{one_list}"
        );
        assert_eq!(one_list["written"]["errorStyle"], "stop");
        assert_eq!(
            one_list["formulas"],
            json!(["\"红,黄,绿\"", "0"]),
            "formula2 是它补的默认值"
        );
        assert_eq!(
            theirs["validations"]["list"][2]["formulas"],
            json!(["ISNUMBER(B2)", "0"])
        );
        assert_eq!(
            theirs["conditional"][1]["rule_list"][0]["scale"]["colors"],
            json!(["FFFFFFFF", "FFFFEB84", "FFF8696B"]),
            "同一种色，alpha 那两位两家写法不同"
        );

        // 第二张表两样都没有：报 0 与「没写」，而不是缺键
        let clean = &hand["sheets"][1]["rules"];
        assert_eq!(
            clean["conditional"].as_array().expect("是数组").len(),
            0,
            "{clean}"
        );
        assert_eq!(clean["validations"]["written"], Value::Null);
        assert_eq!(clean["validations"]["found"], 0);
        assert_eq!(clean["validations"]["whole"], json!(true));
    }

    /// 窗口的状态与页眉页脚：同一个开关两种拼法，「没写」与「写了空的」更是两回事
    #[test]
    fn views_and_headers_report_only_what_the_sheet_written() {
        let hand = run("view.xlsx");
        let first = &hand["sheets"][0];
        assert_eq!(
            first["view"]["written"],
            json!({"showGridLines": "0", "tabSelected": "1", "zoomScale": "150", "workbookViewId": "0"}),
            "只交文件写了的那四个开关：{first}"
        );
        assert_eq!(first["view"]["count"], 1);
        assert_eq!(first["view"]["pane"]["state"], "frozen");
        assert_eq!(first["view"]["pane"]["topLeftCell"], "B3");
        assert_eq!(
            first["view"]["selections"]
                .as_array()
                .expect("是数组")
                .len(),
            3,
            "第一家三条 selection 且不写 pane=\"topLeft\"：{first}"
        );
        let head = &first["header_footer"];
        assert_eq!(head["written"], json!({"differentOddEven": "1"}), "{head}");
        assert_eq!(head["present"], json!(true));
        assert_eq!(head["written_slots"], 3);
        let odd = &head["slots"][0];
        assert_eq!(odd["element"], "oddHeader");
        assert_eq!(
            odd["text"], "&L第 &A 页&C冻结那张",
            "实体解掉之后的原样：{odd}"
        );
        assert_eq!(
            odd["segments"],
            json!([{"at": "left", "text": "第 &A 页"}, {"at": "center", "text": "冻结那张"}]),
            "分段只交文件自己标出来的那几段：{odd}"
        );
        assert_eq!(odd["fields"], json!(["&A"]));
        let foot = &head["slots"][1];
        assert_eq!(foot["element"], "oddFooter");
        assert_eq!(
            foot["fields"],
            json!(["&&", "&P", "&N"]),
            "那个 && 是一个 &，不是标记：{foot}"
        );
        assert_eq!(
            foot["segments"][0],
            json!({"at": "left", "text": "打开 && 关闭"})
        );
        assert_eq!(
            foot["segments"][1],
            json!({"at": "right", "text": "第 &P 页，共 &N 页"})
        );
        // 写了但是空的那个段落，与「这个元素整个没有」是两件事
        assert_eq!(head["slots"][3]["present"], json!(true));
        assert_eq!(head["slots"][3]["text"], "");

        let split = &hand["sheets"][1];
        assert_eq!(
            split["view"]["pane"]["state"], "split",
            "拆分与冻住在同一个键上：{split}"
        );
        assert_eq!(split["header_footer"]["present"], json!(false));
        assert_eq!(split["header_footer"]["written"], Value::Null);
        assert_eq!(split["header_footer"]["slots"][0]["present"], json!(false));
        assert_eq!(split["header_footer"]["slots"][0]["text"], Value::Null);
        assert_eq!(split["header_footer"]["written_slots"], 0);

        let plain = &hand["sheets"][2];
        assert_eq!(
            plain["view"]["pane"],
            Value::Null,
            "第三张表没冻也没拆：{plain}"
        );
        assert_eq!(
            plain["view"]["selections"]
                .as_array()
                .expect("是数组")
                .len(),
            1
        );

        // LibreOffice 重写同一个文件：开关换成 true/false 并补出十几个属性、selection
        // 变成四条还带 activeCellId、每段字前面多一个 &"Calibri"，而那个「拆分」的 pane
        // 整个不见了（同一份件冻住的那张还留着）
        let lo = run("view-lo.xlsx");
        let mine = &lo["sheets"][0];
        assert_eq!(
            mine["view"]["written"]["showGridLines"], "false",
            "同一条开关的第二种拼法：{mine}"
        );
        assert_eq!(
            mine["view"]["written"].as_object().expect("是对象").len(),
            15,
            "它把没写的开关也全写出来：{mine}"
        );
        assert_eq!(mine["view"]["pane"]["state"], "frozen");
        assert_eq!(
            mine["view"]["selections"].as_array().expect("是数组").len(),
            4
        );
        assert_eq!(
            mine["header_footer"]["written_slots"], 3,
            "两份件写了字的段数一样"
        );
        assert_eq!(
            mine["header_footer"]["written"]["differentFirst"], "false",
            "openpyxl 干脆不写这个开关：{mine}"
        );
        let theirs = &mine["header_footer"]["slots"][0];
        assert_eq!(
            theirs["text"], r#"&L&"Calibri"第 &A 页&C&"Calibri"冻结那张"#,
            "{theirs}"
        );
        assert_eq!(
            theirs["fields"],
            json!([r#"&"Calibri""#, "&A", r#"&"Calibri""#]),
            "字体码每段前面一个：{theirs}"
        );
        assert_eq!(
            theirs["segments"][0],
            json!({"at": "left", "text": r#"&"Calibri"第 &A 页"#})
        );
        assert_eq!(
            lo["sheets"][1]["view"]["pane"],
            Value::Null,
            "LO 的 xlsx 导出把 split 那个 pane 整个丢了"
        );
        // 它给每张表都写出 headerFooter，哪怕里面是空的
        assert_eq!(lo["sheets"][2]["header_footer"]["present"], json!(true));
        assert_eq!(lo["sheets"][2]["header_footer"]["written_slots"], 0);
        assert_eq!(lo["sheets"][2]["header_footer"]["slots"][0]["text"], "");
        assert_eq!(
            lo["sheets"][2]["header_footer"]["slots"][2]["present"],
            json!(false),
            "even 那一对它又省掉了：两份件的 present 各按各的"
        );

        // ODS 与 .xls 这一族的窗口与页眉页脚没读：键整个不在
        for name in ["book.ods", "hidden.ods", "cell-notes.ods", "book.xls"] {
            for one in run(name)["sheets"].as_array().expect("是数组") {
                assert!(one.get("view").is_none(), "{name} 这一族的窗口没读：{one}");
                assert!(
                    one.get("header_footer").is_none(),
                    "{name} 这一族的页眉页脚没读：{one}"
                );
            }
        }
    }

    /// 列宽、行高、筛选与表对象：两家的数互不相等，重写一次就换一套换算
    #[test]
    fn sizes_filters_and_table_objects_come_from_the_sheet_itself() {
        let hand = run("size.xlsx");
        let first = &hand["sheets"][0];
        assert_eq!(
            first["layout"]["format"],
            json!({"baseColWidth": "8", "defaultRowHeight": "18"}),
            "「默认列宽」这一家写的是 baseColWidth：{first}"
        );
        assert_eq!(first["layout"]["columns"]["written"], 2);
        assert_eq!(first["layout"]["columns"]["covered"], 2);
        assert_eq!(first["layout"]["columns"]["list"][0]["width"], "22.5");
        assert_eq!(first["layout"]["columns"]["list"][1]["hidden"], "1");
        assert_eq!(first["layout"]["rows"]["elements"], 3);
        assert_eq!(
            first["layout"]["rows"]["with_height"], 2,
            "只有一行没说过话"
        );
        assert_eq!(
            first["layout"]["rows"]["list"][0],
            json!({"r": "2", "ht": "40", "customHeight": "1"}),
            "{first}"
        );
        let filter = &first["filter"];
        assert_eq!(filter["present"], json!(true));
        assert_eq!(filter["written"], json!({"ref": "A1:C3"}), "{filter}");
        assert_eq!(filter["mode"], Value::Null, "这一家不写 filterMode");
        assert_eq!(
            filter["columns"][0]["written"],
            json!({"colId": "0", "hiddenButton": "0", "showButton": "1"}),
            "{filter}"
        );
        assert_eq!(filter["columns"][0]["vals"], json!(["甲"]));
        assert_eq!(first["tables"], 1);
        let table = &first["table_list"][0];
        assert_eq!(table["part"], "xl/tables/table1.xml", "{table}");
        assert_eq!(
            table["written"]["ref"], "A1:B3",
            "表对象的范围与筛选那一条的范围不是一回事：{table}"
        );
        assert_eq!(table["written"]["name"], "台账");
        assert_eq!(
            table["columns"]["names"],
            json!(["一月", "10"]),
            "列名是文件自己写的：{table}"
        );
        assert_eq!(table["columns"]["whole"], json!(true));
        assert_eq!(
            table["style"],
            json!({"name": "TableStyleMedium2", "showRowStripes": "1"}),
            "{table}"
        );
        assert_eq!(
            first["table_parts"],
            json!({"written": "1", "found": 1, "whole": true, "resolved": 1}),
            "{first}"
        );

        // 第二张表什么都没收
        let plain = &hand["sheets"][1];
        assert_eq!(plain["layout"]["columns"]["written"], 0);
        assert_eq!(
            plain["layout"]["columns"]["covered"],
            Value::Null,
            "一条 col 也没有，盖住几列判不住：{plain}"
        );
        assert_eq!(plain["layout"]["rows"]["elements"], 0);
        assert_eq!(plain["filter"]["present"], json!(false));
        assert_eq!(plain["filter"]["mode"], Value::Null);
        assert_eq!(plain["tables"], 0);
        assert!(plain["table_list"].as_array().expect("是数组").is_empty());

        // LibreOffice 重写同一个文件：换算换了一套、每行都写高度、filterMode 自己补上，
        // 而 tableParts 那个 count 它不写
        let lo = run("size-lo.xlsx");
        let mine = &lo["sheets"][0];
        assert_eq!(
            mine["layout"]["format"]["defaultColWidth"], "7.7734375",
            "另一家的「默认列宽」是另一个属性名：{mine}"
        );
        assert_eq!(mine["layout"]["format"]["baseColWidth"], Value::Null);
        assert_eq!(
            mine["layout"]["columns"]["list"][0]["width"], "20.47",
            "同一列换一家是 20.47：{mine}"
        );
        assert_eq!(mine["layout"]["columns"]["list"][1]["hidden"], "true");
        assert_eq!(
            mine["layout"]["rows"]["with_height"], 3,
            "它给每行都写了高度"
        );
        assert_eq!(
            mine["layout"]["rows"]["list"][1]["ht"], "39.75",
            "40 换一家是 39.75"
        );
        assert_eq!(
            mine["filter"]["mode"], "true",
            "筛着的时候它另写一条 filterMode"
        );
        assert_eq!(
            mine["filter"]["columns"][0]["written"],
            json!({"colId": "0"}),
            "那两个开关它不写：{mine}"
        );
        let theirs = &mine["table_list"][0];
        assert_eq!(theirs["written"]["name"], "台账", "表名一字未变");
        assert_eq!(theirs["columns"]["names"], json!(["一月", "10"]));
        assert_eq!(
            theirs["written"]["totalsRowCount"], "0",
            "totals 那两个开关它补上了：{theirs}"
        );
        assert_eq!(
            theirs["style"].as_object().expect("是对象").len(),
            5,
            "{theirs}"
        );
        assert_eq!(
            mine["table_parts"]["written"],
            Value::Null,
            "count 它不写：{mine}"
        );
        assert_eq!(mine["table_parts"]["found"], 1);
        assert_eq!(
            lo["sheets"][1]["filter"]["mode"], "false",
            "没筛的表它也写一条 false"
        );

        // ODS 读了尺寸这一族（每一张表都被补到 16384 列，见 ods 那两条测试），
        // 但筛选与表对象是 OOXML 才有的东西；.xls 三份都没读 —— 都是「键整个不在」
        for name in ["book.ods", "hidden.ods"] {
            for one in run(name)["sheets"].as_array().expect("是数组") {
                let spans = one["layout"]["columns"]["spans"].as_u64();
                assert_eq!(spans, Some(16384), "每张表都被补到整 16384 列：{one}");
                assert!(one.get("filter").is_none(), "这一族的筛选没读：{one}");
                assert!(one.get("tables").is_none(), "这一族的表对象没读：{one}");
            }
        }
        for one in run("book.xls")["sheets"].as_array().expect("是数组") {
            assert!(one.get("layout").is_none(), ".xls 这一族的尺寸没读：{one}");
            assert!(one.get("filter").is_none(), ".xls 这一族的筛选没读：{one}");
            assert!(
                one.get("tables").is_none(),
                ".xls 这一族的表对象没读：{one}"
            );
        }
    }

    /// 不是表格的文件要指路
    #[test]
    fn a_word_document_is_not_a_sheet() {
        let app = OfficeSheet {
            path: PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("tests/fixtures/office/notes.docx"),
            limit: 10,
            max_bytes: 1 << 20,
            csv: false,
            sheet: String::new(),
        };
        let (tx, _rx) = mpsc::channel();
        let why = run_office_sheet(&app, &Context::new_test(tx)).unwrap_err();
        assert!(why.to_string().contains("office-doc"), "{why}");
    }

    /// .ods 的页眉页脚只在**页版式**上：`Report` 那份页眉的两段字一边一个住在
    /// `style:region-left` / `-right` 里（只走直接孩子就读成 0 段、空串），而全文没有
    /// 一条写着的属性把某张表连到某份页版式 —— 所以这份账按页版式交，不假装有归属。
    /// 期望值全部来自 `office_reader.py:odf_page_styles` 对同一批件的读数
    #[test]
    fn a_spreadsheet_header_lives_in_two_halves_of_a_page_style() {
        let hand = run("book.ods");
        let pages = &hand["page_styles"];
        assert_eq!(pages["family"], "odf");
        assert_eq!(pages["available"], json!(true));
        assert_eq!(pages["masters_total"], json!(5));
        // 页版式与表的归属：没有一条写着的属性可走，所以五份全都「没人点它的名」
        assert_eq!(pages["sections_total"], json!(0));
        assert_eq!(pages["masters_named_by_section"], json!(0));
        assert_eq!(pages["masters_unnamed"], json!(5));
        let report = &pages["masters"][1];
        assert_eq!(report["name"], "Report");
        let header = &report["slots"]["header:default"];
        // 两半各一段，合起来才是这一格的全部：只走直接孩子这两个数都是 0
        assert_eq!(header["paragraphs"], json!(2));
        assert_eq!(header["text"], "???(???)\n0000/00/00, 00:00:00");
        let regions = header["regions"].as_array().expect("是数组");
        assert_eq!(regions.len(), 2, "{header}");
        assert_eq!(regions[0]["element"], "style:region-left");
        assert_eq!(regions[0]["paragraphs"], json!(1));
        // 表名与标题是域，文件里缓存的是三个问号：按原样交，不替它算
        assert_eq!(regions[0]["text"], "???(???)");
        assert_eq!(regions[1]["element"], "style:region-right");
        assert_eq!(regions[1]["text"], "0000/00/00, 00:00:00");
        assert_eq!(header["fields"]["date"], json!(1));
        assert_eq!(header["fields"]["time"], json!(1));
        assert_eq!(report["slots"]["footer:default"]["text"], "页 1/ 99");
        // 写了这一格而它自己说不显示：与整个没有这一格（null）是两份不同的文件
        let quiet = &pages["masters"][2];
        assert_eq!(quiet["name"], "PageStyle_5f_说明");
        assert_eq!(quiet["slots"]["header:default"]["present"], json!(true));
        assert_eq!(quiet["slots"]["header:default"]["paragraphs"], json!(0));
        assert_eq!(quiet["slots"]["header:default"]["display_written"], "false");
        assert_eq!(
            quiet["slots"]["header:default"]["regions"]
                .as_array()
                .expect("是数组")
                .len(),
            0
        );
        // 没写这一开关的就是没写：null，不是 false
        assert!(pages["masters"][0]["slots"]["header:default"]["display_written"].is_null());
        assert!(
            pages["masters"][0]["slots"]["header:first"]["display_written"].as_str()
                == Some("false")
        );
        // OOXML 那一支没有这一份账：页版式是 ODF 的事，缺键就是没读这一族
        assert!(run("book.xlsx")["page_styles"].is_null());
    }

    /// 「这张表打哪几行几列、每页重复哪一行」在两族是两处地方，而且归属的给法完全不同：
    /// OOXML 把它写成 `xl/workbook.xml` 的两条保留名，靠 `localSheetId` 那个**顺序号**落到表上；
    /// ODF 把它写成 `table:table` 自己身上的一条属性，另有一份为与 Excel 来回而写的 `named-*`。
    /// 期望值全部来自 `office_reader.py` 的 `xlsx_print_ranges` / `ods_print_ranges`
    #[test]
    fn what_a_sheet_prints_is_written_in_two_places_per_family() {
        let hand = run("print-area.xlsx");
        let ooxml = &hand["print_ranges"];
        assert_eq!(ooxml["family"], "ooxml");
        assert_eq!(ooxml["available"], json!(true));
        assert_eq!(ooxml["sheets_total"], json!(4));
        assert_eq!(ooxml["defined_total"], json!(5));
        assert_eq!(ooxml["print_entries"], json!(5));
        // 序号 0..3 全落得地：那三套号（顺序、sheetId、r:id）各自编，只有这一个能用
        assert_eq!(ooxml["unresolved"], json!(0));
        assert_eq!(ooxml["quoted_entries"], json!(5));
        assert_eq!(
            ooxml["by_name"],
            json!({"_xlnm.Print_Area": 3, "_xlnm.Print_Titles": 2})
        );
        assert_eq!(ooxml["entries"][0]["resolved_sheet"], "区域与标题");
        // 一条 definedName 里塞两段：分隔符是逗号，摊开成两段而原句另交
        assert_eq!(
            ooxml["sheets"][2]["area_ranges"]
                .as_array()
                .expect("是数组")
                .len(),
            2
        );
        assert_eq!(ooxml["sheets"][2]["area_ranges"][0], "'两段区域'!$A$1:$B$6");
        // 「只给重复列、没给区域」的那一张：两个数各自说自己的话
        assert_eq!(ooxml["sheets"][3]["name"], "什么都没给");
        assert_eq!(ooxml["sheets"][3]["area_entries"], json!(0));
        assert_eq!(ooxml["sheets"][3]["titles_entries"], json!(1));
        assert_eq!(ooxml["sheets"][3]["titles_ranges"][0], "'什么都没给'!$B:$B");

        // 同一条稿子换 LibreOffice 重写：条数一字不差，而引号全没了（写法不是说法）
        let rew = run("print-area-lo.xlsx");
        let lo = &rew["print_ranges"];
        assert_eq!(lo["print_entries"], json!(5));
        assert_eq!(lo["quoted_entries"], json!(0));
        assert_eq!(lo["entries"][0]["local_sheet_id"], json!(2));
        assert_eq!(lo["entries"][0]["resolved_sheet"], "两段区域");
        assert_eq!(
            lo["entries"][0]["text"],
            "两段区域!$A$1:$B$6,两段区域!$C$8:$C$12"
        );

        // ODF 那一族：属性在表身上（分隔符换成空白、地址是点号写法），来回那一份另交一笔
        let ods = run("print-area.ods");
        let pages = &ods["print_ranges"];
        assert_eq!(pages["family"], "odf");
        assert_eq!(pages["tables_total"], json!(4));
        assert_eq!(pages["with_print_ranges"], json!(3));
        assert_eq!(pages["tables"][2]["ranges"][1], "两段区域.C8:两段区域.C12");
        assert!(pages["tables"][3]["print_ranges_written"].is_null());
        assert_eq!(pages["named_total"], json!(5));
        assert_eq!(pages["built_in_total"], json!(5));
        // 同一个选择在一种文件里是两种元素：一段是 named-range，两段是 named-expression
        assert_eq!(pages["named_by_element"]["range"], json!(4));
        assert_eq!(pages["named_by_element"]["expression"], json!(1));
        // 五样的 base-cell-address 全是同一个：归属只在地址串里，不在这条指针上
        assert_eq!(pages["distinct_base_addresses"], json!(1));
        // 「重复行」与「重复列」写的还是同一串，所以只按文件说的交
        assert_eq!(
            pages["usable_as_written"],
            json!({"print-range": 2, "repeat-column repeat-row": 2})
        );

        // 没写过这件事的件：数过了没有（0），不是缺键；.xls 这一族整个不交
        let book = run("book.xlsx");
        assert_eq!(book["print_ranges"]["print_entries"], json!(0));
        assert_eq!(book["print_ranges"]["defined_total"], json!(1));
        assert_eq!(
            run("book.ods")["print_ranges"]["with_print_ranges"],
            json!(0)
        );
        assert!(run("book.xls")["print_ranges"].is_null());
    }

    /// 「公式那枚 `<f>` 自己写了什么」：共享组的跟随格在文件里**没有公式正文**，
    /// 三个生产者三种写法，而 ODF 那一族没有共享组这个位置。
    /// 期望值全部来自 `office_reader.py:xlsx_formula_elems` / `ods_formula_elems`。
    #[test]
    fn a_shared_formula_leaves_seven_cells_with_no_text() {
        let deck = run("shared.xlsx");
        let mine = &deck["formula_elems"];
        assert_eq!(mine["available"], json!(true));
        assert_eq!(mine["family"], "ooxml");
        assert_eq!(mine["sheets_seen"], 1);
        assert_eq!(mine["formula_elems"], 16);
        assert_eq!(mine["with_attrs"], 8, "只有共享组那八枚带属性");
        // attrs_seen 是**文件里属性写的顺序**，不是字典序
        assert_eq!(mine["attrs_seen"], json!(["t", "ref", "si"]));
        assert_eq!(
            mine["attr_values"],
            json!({"t": {"shared": 8}, "si": {"0": 8}, "ref": {"B1:B8": 1}})
        );
        // 「几格有公式」16 与「几格写了正文」9 是两个数
        assert_eq!(mine["text_written"], 9);
        assert_eq!(mine["empty_text"], 7);
        assert_eq!(mine["empty_text_with_cached"], 7);
        assert_eq!(mine["shared_elems"], 8);
        assert_eq!(mine["shared_masters"], 1);
        assert_eq!(mine["shared_followers"], 7);
        assert_eq!(mine["si_written"], 8);
        assert_eq!(mine["ref_written_elems"], 1);
        assert_eq!(mine["cached_elems"], 16);
        let rows = mine["cells"].as_array().expect("是数组");
        assert_eq!(rows.len(), 16);
        // 文档序：一行里先 B 后 C，所以「第几条」不是「第几行」
        assert_eq!(rows[0]["cell"], "B1");
        assert_eq!(rows[0]["text"], "A1*2");
        assert_eq!(rows[0]["ref_written"], "B1:B8");
        assert_eq!(rows[0]["shared"], json!(true));
        assert_eq!(rows[1]["cell"], "C1");
        assert_eq!(rows[1]["shared"], json!(false));
        assert_eq!(rows[1]["si"], Value::Null);
        assert_eq!(rows[2]["cell"], "B2");
        assert_eq!(rows[2]["text"], "");
        assert_eq!(rows[2]["text_written"], json!(false));
        assert_eq!(rows[2]["shared"], json!(true));
        assert_eq!(rows[2]["si"], "0");
        assert_eq!(rows[2]["ref_written"], Value::Null);
        // 它带一枚 `<v>` 标签，可那标签里没有字：「有 v」与「有缓存值」分两格
        assert_eq!(rows[2]["cached_written"], json!(true));
        assert_eq!(rows[2]["cached"], "");

        // LibreOffice 不用共享组，但每条 `<f>` 都写 aca="false"
        let back = run("shared-lo.xlsx");
        let rewrote = &back["formula_elems"];
        assert_eq!(rewrote["formula_elems"], 16);
        assert_eq!(rewrote["with_attrs"], 16);
        assert_eq!(rewrote["attrs_seen"], json!(["aca"]));
        assert_eq!(rewrote["attr_values"], json!({"aca": {"false": 16}}));
        assert_eq!(rewrote["text_written"], 16);
        assert_eq!(rewrote["empty_text"], 0);
        assert_eq!(rewrote["shared_elems"], 0);
        assert_eq!(rewrote["si_written"], 0);
        // openpyxl 那一份一枚属性都不写
        let plain = run("book.xlsx");
        assert_eq!(plain["formula_elems"]["formula_elems"], 1);
        assert_eq!(plain["formula_elems"]["with_attrs"], 0);
        assert_eq!(plain["formula_elems"]["attrs_seen"], json!([]));
        // 一张表公式都没有的件：一串 0 而不是缺键
        let none = run("formats.xlsx");
        assert_eq!(none["formula_elems"]["available"], json!(true));
        assert_eq!(none["formula_elems"]["formula_elems"], 0);
        assert_eq!(none["formula_elems"]["empty_text"], 0);

        // ODF：公式是格子身上的属性，每条都带正文；共享组那几格这一族整个不交
        let ods = run("shared.ods");
        let side = &ods["formula_elems"];
        assert_eq!(side["family"], "odf");
        assert_eq!(side["formula_elems"], 16);
        assert_eq!(side["text_written"], 16);
        assert_eq!(side["empty_text"], 0);
        assert_eq!(side["attrs_seen"], json!(["table:formula"]));
        assert_eq!(side["attr_values"], json!({"formula-prefix": {"of": 16}}));
        assert!(side.get("shared_elems").is_none());
        assert!(side.get("si_written").is_none());
        let cells = side["cells"].as_array().expect("是数组");
        assert_eq!(cells[0]["text"], "of:=[.A1]*2");
        assert_eq!(cells[0]["cached"], "6");
        assert_eq!(cells[0]["paragraphs"], 1);
        assert_eq!(cells[1]["text"], "of:=SUM([.$A$1:.A1])");
        assert_eq!(cells[1]["cached"], "3");
        // 与这条 lane 无关的一份老件也照样有这一格（它有一枚公式、也带缓存值）
        assert_eq!(run("book.ods")["formula_elems"]["formula_elems"], 1);
        assert_eq!(run("book.ods")["formula_elems"]["text_written"], 1);
        // 遗留 .xls 不交这个键（那一族的公式在 BIFF 记录树里，是另一问）
        assert!(run("book.xls")["formula_elems"].is_null());
    }
}
