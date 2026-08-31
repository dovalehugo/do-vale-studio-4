# Development Rules

## Golden Rule

Do not implement a large feature without first defining:

- architecture
- ownership
- threading
- error handling
- performance implications
- tests

---

# Cursor Workflow

For every major task:

1. Read PROJECT_CONTEXT.md.
2. Read ARCHITECTURE.md.
3. Inspect existing implementation.
4. Explain the proposed design.
5. Identify risks.
6. Implement the smallest vertical slice.
7. Run tests.
8. Run benchmarks where relevant.
9. Update documentation.
10. Commit.

---

# Never

Do not:

- rewrite the project without justification
- introduce unnecessary dependencies
- move video rendering into egui
- decode every frame into CPU RGBA
- block the UI thread
- perform synchronous disk IO during playback
- optimize without measurements
- use proxies to hide decoder/rendering problems

---

# Commits

Use small commits.

Examples:

feat(media): add media probing
feat(decoder): add hardware capability detection
feat(gpu): add GPU frame abstraction
feat(render): add render graph foundation
perf(decoder): reduce frame transfer overhead
test(playback): add frame scheduler tests