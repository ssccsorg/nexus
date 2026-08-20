# 159: Workspace refactoring — remove petgraph/composite, split nexus-model into nex-core/nex-fih/nex-storage

## Context

Tagma의 coordinate-space addressing이 도입되면서 petgraph 기반의 graph RAG 인덱싱이 더 이상 필요하지 않게 되었다. Tagma는 좌표 기반의 공간 주소 지정(spatial addressing)을 통해 기존 petgraph가 제공하던 neighborhood query, subgraph traversal, graph projection을 대체한다. 이로 인해 `storage/composite`과 `storage/petgraph` 두 crate를 nexus workspace에서 제거할 수 있는 기반이 마련되었다.

이번 세션은 두 가지 목표를 가진다. 첫 번째는 `storage/composite`(CF Workers 3-tier cold storage)와 `storage/petgraph`(PetgraphStorage hot storage)를 완전히 제거하는 것이다. 두 번째는 syntagma workspace의 crate 분할 패턴(`core/`, `geo/`, `kv/`)을 따라 `nexus-model`을 세 개의 독립 crate로 분할하는 것이다.

분할의 핵심 원칙은 **계층적 의존성**이다. `nex-core`는 순수 저장소 인터페이스만 정의하고 FIH 타입(Fact, Intent, Hint, BoardState)을 전혀 알지 못한다. `nex-fih`는 FIH 타입과 FIH-specific storage traits를 정의하며 `nex-core`에 의존한다. `nex-storage`는 구현체 허브로서 `FihStorage`, `FileIo`, `FihBlackboard` 등의 구체적인 구현을 제공한다.

## Key decisions

### petgraph/composite removal

petgraph RAG 인덱싱은 Tagma의 coordinate-space addressing으로 완전히 대체되었다. composite storage는 Cloudflare Workers의 3-tier architecture(R2 + Durable Objects + KV)에 특화된 구현체였으나, 이 또한 Tagma 기반의 Coord 네이티브 IO로 전환됨에 따라 제거 대상이 되었다.

PetgraphStorage는 `storage/petgraph/`에 위치하며 `GraphRead`, `GraphWrite`, `FactCapable`, `IntentCapable`, `HintCapable`을 구현하고 있었다. 그러나 이 모든 기능은 `FihStorage`(nex-storage)와 Tagma coord 기반 접근으로 대체 가능하다. CompositeColdStorage는 `storage/composite/`에서 3-tier cold storage orchestration을 담당했으나, session-server와 async-store로 핵심 패턴만 추출하고 나머지는 제거되었다.

### FihBlackboard sync implementation

기존 `HybridBlackboard`는 PetgraphStorage(hot) + CompositeColdStorage(cold)를 묶는 복합체였다. petgraph/composite 제거로 HybridBlackboard도 사라졌다. 대신 `FihBlackboard`가 `FihStorage`를 `futures_executor::block_on`으로 감싸는 sync wrapper 역할을 수행한다.

`FihBlackboard`는 `StorageRead`, `FactCapable`, `IntentCapable`, `HintCapable`, `EvictCapable`을 모두 `block_on`을 통해 동기적으로 구현한다. 단, 이는 native 전용(`#[cfg(not(target_arch = "wasm32"))]`)이며 WASM에서는 사용할 수 없다. FihStorage 자체는 순수 async execution unit으로 남아 있어, I/O blocking을 암시하지 않도록 설계되었다.

### model -> nex-core + nex-fih + nex-storage 분할

syntagma의 `core/`, `geo/`, `kv/` 구조를 따라 nexus-model을 세 개의 crate로 분할했다.

| Crate | 책임 | 주요 타입 | 의존성 |
|-------|------|-----------|--------|
| **nex-core** | 순수 저장소 인터페이스 | `Clock`, `BlobStore`, `MetaStore`, `ObjectStore` | 없음 (표준 라이브러리만) |
| **nex-fih** | FIH 타입 및 FIH storage traits | `Fact`, `Intent`, `Hint`, `BoardState`, `Content`, `FactCapable`, `IntentCapable`, `HintCapable`, `StorageRead`, `EvictCapable` | `nex-core`, `tagma-core` |
| **nex-storage** | 구현체 허브 | `FihStorage`, `FihBlackboard`, `FileIo`, `FsIo`, `EntityStore`, `SemanticStore` | `nex-core`, `nex-fih` |

`nexus-model`(model/)은 당분간 유지되지만, 점진적으로 nex-core + nex-fih로 이관되고 최종적으로 제거될 예정이다. 현재는 `legacy.rs`를 통해 이전 타입(`StoredEvent`)을 유지하며 호환성을 보장한다.

### Chton project direction

Chton은 Coord 네이티브 IO 저장소 프로젝트다. `nex-core` + `tagma-core` + `tagma-geo`에만 의존하며 FIH 타입을 알 필요가 없다. mmap 기반의 메모리-디스크 동기화를 통해 serialize/deserialize 비용을 제거하는 것이 핵심 설계 목표다.

Chton은 상품화 가능한 독립 프로젝트로, nexus 생태계 밖에서도 사용할 수 있어야 한다. `ChStorage`는 `StorageRead` + `FactCapable` + `IntentCapable` + `SpatialOps`를 구현하며, Coord 기반의 proximity query와 range query를 tagma-geo를 통해 제공한다.

### USB hub pattern 유지

nexus는 trait 명세(interface)만 정의하고 구체적인 구현은 Chton, DuckDB, Spin KV 등 외부 backend에 위임한다. nexus가 Chton을 직접 알 필요가 없으며, Chton이 `nex-core`와 `nex-fih`의 trait을 구현하기만 하면 된다. 이는 USB hub 패턴과 동일하다.

## File changes summary

| 변경 유형 | 디렉토리 | 라인 수 | 설명 |
|-----------|----------|---------|------|
| 삭제 | `storage/composite/` | 2,087줄 | CF Workers 3-tier cold storage 제거 |
| 삭제 | `storage/petgraph/` | 1,044줄 | PetgraphStorage hot storage 제거 |
| 삭제 | `apps/nex-cf/` | 4,610줄 | CF Workers WASM 애플리케이션 제거 |
| 신규 | `libs/session-server/` | (추출) | sync 직렬화 패턴 (composite store_session에서 추출) |
| 신규 | `libs/async-store/` | (추출) | in-memory BlobStore/MetaStore/ObjectStore 구현 (composite async_store에서 추출) |
| 신규 | `nex-core/` | (신규) | 순수 저장소 인터페이스 crate |
| 신규 | `nex-fih/` | (신규) | FIH 타입 및 storage traits crate |
| 신규 | `nex-storage/` | (신규) | 구현체 허브 crate |
| 수정 | `model/` | 2줄 | nex-storage FihBlackboard로 imports 이관 준비 |
| 수정 | `nex/` | 73줄 | FihBlackboard sync impl 추가, composite/petgraph 의존성 제거 |
| 수정 | `interface/cypher/` | 다수 파일 | composite integration test 제거 |

총 60개 파일 변경, 265줄 추가, 8,685줄 삭제.

## Chton integration outlook

Chton은 nex-core + tagma-core + tagma-geo에만 의존한다. 이는 Chton이 FIH 타입을 전혀 알 필요 없이 순수 저장소 엔진으로 동작할 수 있음을 의미한다.

주요 통합 지점:

| 구성 요소 | 역할 |
|-----------|------|
| `ChStorage` | `StorageRead` + `FactCapable` + `IntentCapable` + `SpatialOps` 구현 |
| Coord-based query | tagma-geo를 활용한 proximity query, range query, spatial join |
| mmap sync | 메모리-디스크 직접 매핑으로 serialize/deserialize 불필요 |
| Import/export | `FihExport`, `FihImport` trait을 통해 기존 FihStorage ↔ Chton 간 마이그레이션 |

첫 적용 대상은 `rem`의 `write_checkpoint` / `restore_from_snapshot`이다. 현재 rem은 FihStorage의 `export_from_io` / `import_into_io`를 사용하는데, Chton 기반으로 전환하면 mmap된 Coord 영역을 직접 checkpoint 파일에 매핑하는 방식으로 단순화된다.

```rust
// Chton이 제공할 API (설계 단계)
pub trait ChStorage: StorageRead + FactCapable + IntentCapable + SpatialOps {
    fn coord_region(&self) -> &CoordRegion;
    fn checkpoint(&self, path: &str) -> Result<(), IoError>;
    fn restore(path: &str) -> Result<Self, IoError>
        where Self: Sized;
}
```

### Known gaps

- Chton은 아직 프로토타입 단계로, 현 시점에서는 `FihStorage` + `FileIo`가 주 storage path이다.
- `model/` 디렉토리가 여전히 존재하며 `legacy.rs`를 포함한다. 이는 `StoredEvent` 타입이 `playbooks/agents/` 등에서 사용되고 있기 때문이다. nex-core/nex-fih로의 완전한 이관 이후 제거 예정.
- `FihBlackboard`가 `nex/`와 `nex-storage/` 두 곳에 중복 존재한다. `nex/` 쪽은 `nexus_model` import를 사용하고, `nex-storage/` 쪽은 `nex_fih` import를 사용한다. nex-storage 버전으로 통일 후 nex 버전 제거 예정.
