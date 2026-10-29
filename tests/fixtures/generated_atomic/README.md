# 생성 TABLE의 쪽 소유 기준

`atomic-row-cases-without-line-cache.hwpx`는 검증한 입력의 변경 없는 사본이다.
`atomic-row-cases-without-line-cache.pdf`는 같은 입력을 Hancom Docs web에서 편집 없이
열어 전체 쪽을 내보낸 독립 기준이다. 두 파일의 대응은 파일명 추정이 아니라 입력 해시와
실제 다섯 구역의 내용으로 확인했다. PDF는 10쪽이며 쪽 크기는 595 × 300pt이다.
제품의 세부 빌드 번호는 기록되지 않았다.

| 파일 | SHA-256 |
| --- | --- |
| HWPX | `24d51fd0d926e9ebfda59a60164eece612324d046bbea7b6c5fcdf03f60d5bdf` |
| PDF | `5eb229db67ac2bfe6110c500c61681aca6165d8a0686831d96852b5d8930839a` |
| environment.json | `e59554ba140380fad75ea0cab85b3828b11a39a6c5d341c48bd1ee2333f4e7af` |

입력은 합성 문서다. 최초 단일 구역 원문 `flow-long-cell-hwpx.hwpx`의 해시는
`bbd27d1b40baed5abde4752ac54a4a085b8667d0f6e529415f280c606388cf35`이다.
결합 생성기 `generate.py`의 해시는
`73a3fd46d4b6f1e0d1f7ae8dcdc9f6e32614d3c21037f5adef88601984398080`이다.
생성기는 원문 구역을 다섯 번 복사하고 package의 구역 수·참조·미리보기 텍스트를 갱신했다.
각 구역의 차이는 아래와 같고, 모두 non-TAC·TABLE 정책과 200개 토큰을 보존한다.

| 구역 | 원문 대비 작성한 차이 | 독립 PDF의 앵커 / 표 쪽 |
| --- | --- | --- |
| 1 | 차이 없는 한 행 두 셀 대조군 | 1 / 2 |
| 2 | 오른쪽 셀 rowspan=2, 높이 3600HU; 왼쪽 둘째 행에 Label1, 높이 1800HU 추가 | 3 / 4 |
| 3 | 같은 host 문단의 표 바로 앞에 `HostText ` 삽입 | 5 / 6 |
| 4 | 표 세로 오프셋만 0에서 300HU로 변경 | 7 / 8 |
| 5 | holdAnchorAndSO만 0에서 1로 변경 | 9 / 10 |

결합 직후 원문 해시는 `60b309601ed0bf15ff88a96ff8b40b661860c1805b388abe6cd03216d79c6db5`이다.
그 합성 원문의 수동 LineSeg 가정은 독립 출력과 맞지 않았다. 현재 입력은
`Contents/section0.xml`부터 `section4.xml`까지 각각 두 개의 `hp:linesegarray`를
정확한 바이트 범위로 제거한 별도 입력이다. 총 10개 배열 외에는 XML 바이트와 namespace
접두사를 바꾸지 않았고, 다른 ZIP member payload와 member 순서도 그대로 보존했다.
원래 입력·기준·실패 증거는 별도 보존한다. 현재 입력의 통과는 원래 입력의 수용이나
일반적인 저장 캐시 수정의 증거가 아니다.

독립 PDF에서 `Following body marker`는 각 앵커 쪽에 한 번만 있고 `HostText`는 5쪽에만 있다.
표는 다음 물리 쪽의 새 본문 원점에서 하나의 완전한 객체로 시작한다. 오프셋과 hold 변형에도
원점이 같고, rowspan의 Label0·Label1은 각자의 왼쪽 셀에 있다. 표 전체는 종이 끝을 넘는다.
원문과 렌더 트리는 segment001..segment200을 모두 보존하지만 실제 종이에 기준선이 들어오는
토큰은 001..036이다. 긴 줄의 가로 잉크가 셀 상자를 넘는 현상을 셀 소유 상실로 취급하지 않는다.

추가 전 최종 combined01 Native와 fresh WASM의 전체 10쪽에서
`tolerant_content_match_percent` 최저값은 각각 100%였고 누락 쪽은 없었다.
양쪽의 10개 트리와 10개 raster는 각각 바이트가 같았으며 대표 review·overlay를 직접 판독했다.
Native 바이너리 해시는 `45515934137d2b820a86b84ae52b0a6453d43094904dca5b8fcc130939ff84c8`,
WASM 해시는 `ffe245b770cf46d309ca3cca9368c961d3012dd8497cacd7b00eb95b1d00233f`이다.
검증 source manifest 해시는 `46257f135a771ad4d4549b27bbd7362287a3b333f5dad3cf735f6b7d20df735a`이다.
이는 2px 이웃 관용 내용 실루엣 지표이며 모든 글리프의 완전한 피델리티 점수가 아니다.
대표 strict proxy 값은 약 22.49..31.83%였다.

`environment.json`은 기존 table_spacing 호출자 환경과 바이트가 같다.
글꼴 파일은 포함하지 않는다. 기대값은 독립 PDF의 쪽 소유와 원문 셀·HU 관계에서 정하며
렌더러 내부 helper나 전체 출력 snapshot을 정답으로 사용하지 않는다.
