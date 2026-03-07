# Audit Findings

**Last Updated:** 2026-03-08 10:00 NZ
**Auditor:** Arc (OpenClaw Agent)

---

## 2026-03-08 Audit

### Commits Reviewed

#### tokio-pgqueue (2 commits)
1. **9b7cbc2** - docs: add integration report for Skilt and xero-ai-forecaster
2. **051ff5e** - fix(docs): use Self::handler_fn for intra-doc link

#### polars-ta (1 commit)
1. **5fce86a** - fix(clippy): remove unnecessary clones, useless into(), add allow attributes

### Issues Found & Fixed

#### polars-ta: Clippy Warnings
**Status:** FIXED
**Files:** src/adx.rs, src/psar.rs, src/mfi.rs, src/trix.rs, src/tsi.rs

**Issues:**
1. `clone_on_copy` - Using `.clone()` on `EWMOptions` which implements `Copy`
2. `useless_conversion` - Using `.into()` on `&str` when `&str` is already the target type
3. `needless_range_loop` - Loop variable `i` used only to index into arrays

**Fixes Applied:**
- Removed unnecessary `.clone()` calls (adx.rs)
- Removed useless `.into()` conversions (psar.rs)
- Added `#[allow(clippy::needless_range_loop)]` where index-based iteration is clearer than enumerate (mfi.rs, trix.rs, tsi.rs, psar.rs)

**Commit:** 5fce86a - pushed to master

### Notes

- krypto: No new commits since 2026-03-07 audit
- krypto has upstream dependency warning (binance-rs-async) for Rust 2024 compatibility - not actionable
- tokio-pgqueue: Clippy clean with Rust 1.93.1
- polars-ta: Clippy clean with Rust 1.93.1

---

## 2026-03-07 Audit (Historical)

### Commits Reviewed

#### krypto (3 commits since 2026-03-06)
1. **af3ef17** - style: fix clippy warnings (unused var, useless into)
2. **261944e** - feat(examples): add simple strategy grid search with directional accuracy validation
3. **31a4f18** - feat(examples): add mean-reversion test with directional accuracy validation

#### krypto-web (1 commit)
- **38d0f43** - fix(frontend): resolve all lint errors and type issues (7 errors, 9 warnings fixed)

#### Skilt-Client-Portal (1 commit)
- **49f9bd5** - fix(api): remove unused request params in GET route handlers

#### xero-ai-forecaster (1 commit)
- **b3a2df92** - Fixes GST reconciliation PIT reconstruction and historical AR/AP aging

### Issues Found

#### krypto: Dependency Version Conflicts
**Status:** FIXED
**File:** `Cargo.toml`
**Issue:** Transitive dependencies require rustc 1.80+, but system had rustc 1.75.0.

**Fix Applied:** 
- Pinned `url = "=2.5.0"` in Cargo.toml
- Downgraded via `cargo update`

**Root cause:** New rust ecosystem crates have bumped MSRV requirements significantly.

**Recommendation:** Upgrade system rustc to 1.93+ (now done via rustup)

#### krypto: validator.rs - Experimental Module
**Status:** DOCUMENTED (Technical Debt)
**File:** `src/backtest/validator.rs`
**Issue:** Large module with many `todo!()` macros and `#[allow(dead_code)]` attributes.
**Impact:** Low - This is intentional. The module is clearly marked as experimental.
**Recommendation:** Complete implementation when intra-bar stop validation becomes priority.

#### krypto-web: setState-in-effect Anti-pattern
**Status:** FIXED
**File:** `frontend/components/StrategyList.tsx`
**Issue:** `useEffect` with synchronous `setState` violates React best practices.
**Fix Applied:** Replaced with `useMemo` + derived state pattern.

#### krypto-web: TypeScript `any` Types
**Status:** FIXED
**Files:** Multiple components
**Fix Applied:** Replaced `any` with proper types (`unknown` + type guards).

#### Skilt-Client-Portal: Unused Route Parameters
**Status:** FIXED
**Files:** `app/api/routing/query/route.ts`, `app/api/staff/route.ts`
**Fix Applied:** Removed unused `request` parameter from GET handler signatures.

---

## Code Quality Observations

### Positive Findings
1. **Auto-Tune web**: Clean - no `any` types, no `console.log`, proper error handling
2. **krypto examples**: Well-structured with proper documentation
3. **Skilt routing**: Proper implementation of routing context and staff APIs
4. **polars-ta**: Clippy clean, well-documented, comprehensive tests
5. **tokio-pgqueue**: Clippy clean, excellent documentation, integration report added

### Areas for Improvement
1. **krypto dependencies**: Consider pinning all transitive dependencies
2. **binance-rs-async**: Upstream Rust 2024 warning - not actionable locally
3. **Error handling**: Consider more robust error handling in krypto examples
