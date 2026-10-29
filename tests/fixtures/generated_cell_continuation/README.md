# 생성 셀 이어받기와 원문 글꼴 누락 회귀 자료

`long-cell.hwp`와 `long-cell.hwpx`는 Label0, segment001부터 segment200,
Following body marker만 담은 합성 원본이다. CELL 분할(값 2)을 선언한 1행 2열
표의 오른쪽 셀이 여러 쪽으로 이어진다. HWP의 오른쪽 긴 셀 문단에는 저장 LineSeg가 없다.
원본 글꼴 슬롯은 비어 있으며 파일 바이트를 수정하지 않았다.

`long-cell-hwp.pdf`와 `long-cell-hwpx.pdf`는 사용자가 같은 입력을 Hancom Docs
웹에서 인쇄하여 제공한 독립 기준이다. 두 PDF는 7쪽이며 1~6쪽에는 각 32개,
7쪽에는 8개의 segment가 순서대로 한 번씩 나온다. Label0는 1쪽에만,
Following body marker는 7쪽 표 아래에만 나온다. PDF Producer는 cairo 1.15.12이며
웹 애플리케이션 버전과 경고는 제공되지 않았다.

`environment.json`은 검증에 사용한 호출자의 HCR Dotum fallbackFace와
trueTypeFaces 선언이다. 원본의 누락된 face 선언을 수정하거나 글꼴 설치를 증명하지
않는다. 글꼴 바이너리는 포함하지 않는다. 공개 한컴 HANDotum.ttf의 hmtx에서
확인한 폭과 파일 SHA-256은 `provenance.json`에 별도로 기록했다.

검사 기대값은 독립 PDF의 내용과 쪽 소유에서 정했다. 기하 검사는 셀 및 본문 내부
포함, 컷 소유 유닛의 연속성, 예약 높이와 실제 표 높이, 뒤 문단의 순서를 비교한다.
현재 구현의 절대 표 높이나 SVG 해시를 정답으로 복사하지 않는다. 모든 원본과
독립 PDF의 해시, 호출자 환경, 시각 선행 검증 수치는 `provenance.json`에 있다.
Native와 fresh WASM에서 같은 원본/독립 기준의 전쪽이 90% 이상인 것을 확인한 뒤
추가했다. 이 Rust 검사는 내용/기하 계약이며 별도 Visual Sweep을 대체하지 않는다.

실행 전 `node scripts/rust-test-suite-manifest.mjs --prepare`로 생성 suite를 준비한다.
해당 suite에서 `generated_cell_continuation::`와 `font_environment_fallback::`를
필터링한다. 생성 suite/manifest 변경은 제안 패치에 포함하지 않는다.

추가 공백 플래그 검사는 같은 합성 원문의 `use_font_space`만 바꾸어 공개 글꼴 결정
API의 스칼라 폭을 검사한다. 기대값은 HANDotum.ttf의 공백 300/1000em과 기존
반각 500HU 계약에서 정했다. 플래그를 켠 변형에 새 쪽수·좌표·시각 기준을 부여하지
않으며, 원본 PDF를 그 변형의 레이아웃 정답으로 주장하지 않는다. `fontsUsed`는 원문에
없는 face를 새로 선언하는 기능이 아니라 실제 선택 결과를 호스트 탐색에 전달하는
목록으로 검사한다. 미사용 fallback은 명명/내장 face 목록에 추가되면 안 된다.
