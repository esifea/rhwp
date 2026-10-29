# 표 간격과 최종 줄 소유 회귀 자료

`table-spacing-cases.hwpx`는 8개 section의 합성 원본이며 바이트를 수정하지 않았다.
`table-spacing-cases.pdf`는 사용자가 같은 원문을 편집 없이 HancomDocsWeb에서
전쪽 인쇄하여 반환한 독립 기준이다. PDF의 각 쪽은 원문 section 순서에 대응한다.
2~4쪽은 같은 표시 내용을 가지므로 서로의 순열을 내용만으로 구분하지 못한다.
원본·PDF·호출자 환경의 SHA-256과 시각 선행 검증은 `provenance.json`에 있다.

1쪽은 간격 0 대조군이다. 2~4쪽은 300HU 간격과 0/20000/20900HU 선언 너비를
가지며 동일한 완전 셀 격자를 표시한다. 5쪽은 2행2열, 6쪽은 둘째 행 두 열 병합,
7쪽은 좌우 150HU 바깥여백, 8쪽은 같은 줄에 놓인 높이가 다른 두 표다.
독립 PDF는 모든 쪽에서 BeforeMarker, FirstCell, DerivedWidthMarker, SecondCell,
FollowingMarker를 각각 한 번 보존한다. Row2Left/Row2Right는 5쪽에만,
Row2MergedCell은 6쪽에만 있다.

기대값은 원문의 10000HU 열·7200HU 행·300HU 내부/외곽 간격과 독립 PDF의
원점/흐름 차이에서 정했다. 한 행 전체 frame은 20900×7800HU, 두 행은
20900×15300HU이며 20300HU 병합 셀은 기존 열 간격을 이미 포함한다.
RenderTree의 앞 셀 할당 상자는 뒤쪽 내부 간격도 포함하므로 셀 상자 사이에
별도의 빈 틈을 요구하지 않는다. 열/행 원점 차이에서 authored 크기를 뺀 값과
마지막 셀 크기로 내부 간격과 행 축소 여부를 검사한다.

`table_spacing_reference.rs`는 공개 API의 최종 트리에서 쪽/표/셀 소유, 내용의
누락·중복, 셀 내부 포함, 원점/전진 차이, 바깥여백 한 번 계상, 같은 줄의 두 표와
뒤 본문 여유를 검사한다. 절대 화면 좌표·SVG 해시·생산 helper 반환값을 정답으로
복사하지 않는다. Native/fresh WASM 전 8쪽 최저 실루엣 100과 root의 6/8쪽
직접 review가 선행했다. Rust 의미/기하 검사는 별도 Visual Sweep을 대체하지 않는다.

`environment.json`은 검증한 호출자 HCR Dotum fallbackFace/trueTypeFaces 선언이다.
원문 face를 수정하지 않으며 글꼴 바이너리를 포함하지 않는다. 원문의 두 공백은
같은 host 줄의 실제 내용과 가로 전진을 가진다.

F1 불일치 행, F3 저장 vpos, 중첩 진단 변형에는 독립 PDF가 없어 정식 검사로
승격하지 않았다. 양의 간격을 가진 분할/반복 머리행/rowspan/caption, 진짜 native
저장 줄, 실제 괘선 가장자리는 미검증이다. 이 8-section 입력에 대응하는 native
HWP/PDF가 없으므로 변환 후 재열기를 주장하지 않는다.

실행 전 `node scripts/rust-test-suite-manifest.mjs --prepare`를 실행한 뒤
`node scripts/run-rust-test.mjs table_spacing_reference -- --cargo-profile release-test --target-dir target/pr-review`
로 검사한다. 생성 suite/manifest는 제안 패치에 포함하지 않는다.
