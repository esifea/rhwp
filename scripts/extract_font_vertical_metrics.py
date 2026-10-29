#!/usr/bin/env python3
import argparse
import hashlib
import io
import json
from pathlib import Path
import re
import sys

from fontTools.ttLib import TTFont
from pypdf import PdfReader


def sha(data):
    return hashlib.sha256(data).hexdigest()


def unicode_advances(font, descendant, program):
    cmap = font['/ToUnicode'].get_object().get_data().decode('latin1')
    mapping = {}
    for block in re.findall(r'beginbfchar(.*?)endbfchar', cmap, re.S):
        for cid, code in re.findall(r'<([0-9a-fA-F]+)>\s*<([0-9a-fA-F]+)>', block):
            if len(code) == 4:
                mapping[int(cid, 16)] = chr(int(code, 16))
    for block in re.findall(r'beginbfrange(.*?)endbfrange', cmap, re.S):
        for low, high, code in re.findall(r'<([0-9a-fA-F]+)>\s*<([0-9a-fA-F]+)>\s*<([0-9a-fA-F]+)>', block):
            for cid in range(int(low, 16), int(high, 16) + 1):
                mapping[cid] = chr(int(code, 16) + cid - int(low, 16))
    ref = descendant.get('/CIDToGIDMap')
    resolved = ref.get_object() if ref is not None else None
    gid_bytes = resolved.get_data() if hasattr(resolved, 'get_data') else None
    advances = {}
    for cid, char in mapping.items():
        gid = int.from_bytes(gid_bytes[2*cid:2*cid+2], 'big') if gid_bytes is not None else cid
        if gid < program['maxp'].numGlyphs and char != '\uffff':
            advances[char] = program['hmtx'][program.getGlyphOrder()[gid]][0]
    return advances


def extract(record, source, root, font_root):
    full = record['kind'] == 'full-font-hhea'
    path = (font_root if full else root) / source['path']
    data = path.read_bytes()
    if sha(data) != source['sha256']:
        raise ValueError(f'입력 해시 불일치: {path}')
    result = dict(source)
    if full:
        program = TTFont(io.BytesIO(data), fontNumber=source['faceIndex'])
        print(f'full-font input: {path.resolve()}', file=sys.stderr)
        result['fontProgramSha256'] = sha(data)
        cmap = program.getBestCmap()
        advances = {chr(c): program['hmtx'][g][0] for c, g in cmap.items()}
    else:
        reader = PdfReader(io.BytesIO(data))
        page = reader.pages[source['firstPage'] - 1]
        matches = []
        for resource, ref in page['/Resources']['/Font'].items():
            font = ref.get_object()
            if str(font.get('/Subtype')) != '/Type0':
                continue
            descendant = font['/DescendantFonts'][0].get_object()
            descriptor = descendant['/FontDescriptor'].get_object()
            stream = descriptor.get('/FontFile2')
            if stream is None:
                continue
            raw = stream.get_object().get_data()
            if sha(raw) == source['programSha256']:
                if str(font.get('/BaseFont')) != source['baseFont'] or str(descendant['/Subtype']) != '/CIDFontType2':
                    raise ValueError('PDF 프로그램 종류/이름 불일치')
                matches.append((str(resource), font, descendant, raw))
        if len(matches) != 1:
            raise ValueError('PDF 프로그램 일치 항목이 하나가 아님')
        resource, font, descendant, raw = matches[0]
        program = TTFont(io.BytesIO(raw))
        advances = unicode_advances(font, descendant, program)
        result.update(fontResource=resource, fontProgramSha256=sha(raw),
                      creator=str(reader.metadata.get('/Creator', '')),
                      producer=str(reader.metadata.get('/Producer', '')),
                      fullFontSha256=None)
    head, hhea = program['head'], program['hhea']
    values = [head.unitsPerEm, hhea.ascent, hhea.descent, hhea.lineGap]
    if values != record['expected']:
        raise ValueError('em/hhea 충돌')
    names = [{'id': n.nameID, 'value': n.toUnicode()} for n in program['name'].names] if 'name' in program else None
    result.update(rawEmHhea=values, nameRecords=names, glyphCount=program['maxp'].numGlyphs,
                  namingLimit='original historical full-source identity unknown; PDF subset name is not identity' if not full else 'current full-font source verified; historical generator source unknown')
    return result, advances


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--root', type=Path, default=Path(__file__).resolve().parents[1])
    parser.add_argument('--font-root', type=Path, required=True)
    parser.add_argument('--check', action='store_true')
    args = parser.parse_args()
    root = args.root.resolve()
    base = root / 'mydocs/tech/font_vertical_metrics'
    spec = json.loads((base / 'qualification.json').read_text())
    width_bytes = (root / 'src/renderer/font_metrics_generated.rs').read_bytes()
    if sha(width_bytes) != spec['historicalWidthSha256']:
        raise ValueError('표 해시 불일치')
    lineage = json.loads((root / 'mydocs/tech/investigations/issue-4964/font_metric_lineage_manifest.json').read_text())
    records, rust = [], ['// scripts/extract_font_vertical_metrics.py가 생성한 선택적 세로 메트릭이다.\n', 'static QUALIFIED_VERTICAL_METRICS: &[QualifiedVerticalMetric] = &[\n']
    for record in spec['records']:
        entry = lineage['entries'][record['entryIndex']]
        identity = entry['metricIdentity']
        if entry['entryId'] != record['entryId'] or identity != dict(name=record['name'], bold=False, italic=False, emSize=record['expected'][0]):
            raise ValueError('기존 메트릭 엔트리 동일성 불일치')
        index = record['entryIndex']
        match = re.search(rf'static FONT_{index}_LATIN_0: \[u16; 95\] = \[(.*?)\];', width_bytes.decode(), re.S)
        widths = [int(v) for v in match[1].split(',') if v.strip()]
        sources, all_advances = [], []
        for source in record['inputs']:
            metadata, advances = extract(record, source, root, args.font_root)
            compared = {c: v for c, v in advances.items() if 32 <= ord(c) <= 126}
            if len(compared) < 5 or any(widths[ord(c)-32] != v for c, v in compared.items()):
                raise ValueError('ASCII 동일성 불일치')
            metadata['matchingAsciiAdvances'] = dict(sorted(compared.items()))
            sources.append(metadata)
            all_advances.append(advances)
        if record['kind'] != 'full-font-hhea':
            if len({s['sha256'] for s in sources}) < 4 or len({s['fontProgramSha256'] for s in sources}) < 4:
                raise ValueError('교차 문서 근거 부족')
            for a in all_advances:
                for b in all_advances:
                    if any(a[c] != b[c] for c in a.keys() & b.keys()):
                        raise ValueError('문서 간 폭 충돌')
        provenance = record['entryId'] + '.hhea.v1'
        records.append(dict(entryIndex=index, entryId=record['entryId'], name=record['name'],
                            acceptedNames=record['acceptedNames'], convention='hhea/source-em',
                            qualification=record['kind'], provenance=provenance, sources=sources,
                            historicalFullFontSha256=None, widthProjectionSha256=entry['semanticHashes']['widthProjectionSha256']))
        em, ascent, descent, gap = record['expected']
        names = ', '.join(json.dumps(n, ensure_ascii=False) for n in record['acceptedNames'])
        rust.append(f'    QualifiedVerticalMetric {{\n        entry_index: {index},\n        name: "{record["name"]}",\n        accepted_names: &[{names}],\n        units_per_em: {em},\n        ascent: {ascent},\n        descent: {descent},\n        line_gap: {gap},\n    }},\n')
    rust.append('];\n')
    outputs = {root/'src/renderer/font_vertical_metrics_generated.rs': ''.join(rust),
               base/'provenance.json': json.dumps(dict(schemaVersion=1, records=records), ensure_ascii=False, indent=2, sort_keys=True)+'\n'}
    for path, text in outputs.items():
        if args.check:
            if path.read_text() != text:
                raise ValueError(f'생성 자료 불일치: {path}')
        else:
            path.write_text(text)
    print(f'{len(records)} qualified records; historical widths unchanged; reproducibility check={args.check}')


if __name__ == '__main__':
    main()
