tb-title = Testbench
tb-group-perf = Performance
tb-group-security = Security
tb-group-correctness = Correctness

tb-sc-big-document = Big document
tb-sc-all-dirty = All dirty
tb-sc-restyle-storm = Restyle storm
tb-sc-markdown = Indented Markdown
tb-sc-localized = Localized rows
tb-sc-many-uis = Many UIs
tb-sc-calc-layout = calc() layout
tb-sc-nested-rules = Nested CSS rules
tb-sc-deep-dom = Deep DOM
tb-sc-var-bombs = var() bombs
tb-sc-grid-bombs = Grid bombs
tb-sc-hostile-args = Hostile Fluent args
tb-sc-args-injection = Args JSON injection
tb-sc-remote-sources = Remote sources
tb-sc-markdown-bombs = Markdown bombs
tb-sc-reuse-vs-fresh = Reuse vs fresh build
tb-sc-indent-vs-flat = Indented vs flat
tb-sc-selector-list = Selector list
tb-sc-text-features = Text features
tb-sc-selector-bombs = Selector bombs
tb-sc-sibling-bombs = Sibling bombs
tb-sc-entity-bombs = Entity bombs
tb-sc-function-bombs = Function bombs
tb-sc-selector-engine = Selector engine
tb-sc-selectors-vs-fresh = Selectors vs fresh build
tb-sc-restyle-vs-full = Scoped vs full restyle
tb-sc-order-independence = Order independence
tb-sc-idle-quiet = Idle stays quiet
tb-sc-scaling = Scaling N vs 4N

tb-desc-big-document = One document of N rows; churn changes about one row in a hundred per frame, so unchanged rows reuse the previous build.
tb-desc-all-dirty = Every row changes every frame: the full render, parse, cascade and diff.
tb-desc-restyle-storm = A context var() recolours every row and a highlight moves each frame: restyles in place.
tb-desc-markdown = A Markdown template with indented Tera blocks, re-rendered every frame.
tb-desc-localized = N rows of Fluent messages in a Markdown bundle, cached and per-value paths, arguments changing per frame.
tb-desc-many-uis = N separate small UIs; churn updates one of them per frame.
tb-desc-calc-layout = N cards sized by mixed-unit calc(), resolved per frame against the stage.
tb-desc-nested-rules = A stylesheet of 20 000 nested rules: its load must fail cleanly instead of aborting the app.
tb-desc-deep-dom = 5 000 nested elements: content past 100 levels is dropped and the rest builds.
tb-desc-var-bombs = var() chains, doubling fallbacks, cycles and nested functions: the substitution stays bounded.
tb-desc-grid-bombs = Grid track counts that used to overflow: the declarations drop, nothing panics.
tb-desc-hostile-args = Fluent string arguments carrying HTML, Markdown links and images and crafted markers: shown as text, nothing forged.
tb-desc-args-injection = Arguments built two ways from a hostile name: the Tera map literal keeps it whole, the hand-written JSON splice is injectable.
tb-desc-remote-sources = An https:// include and border-image: the include fails its load and the image is skipped.
tb-desc-markdown-bombs = Pathological Markdown: deep emphasis, brackets and quotes, crafted class markers, nested indented Tera blocks.
tb-desc-reuse-vs-fresh = A UI mutated at random every frame is compared with a fresh build of the same state every few frames.
tb-desc-indent-vs-flat = An indented and a flat Markdown template of the same document must build the same UI.
tb-desc-selector-list = N rows in a container, plus a list and a table, styled by structural pseudo-classes, sibling combinators, attribute selectors and :not(); about one row in a hundred changes per frame, and the unchanged rows must keep reusing.
tb-desc-text-features = text-align, line-height, letter-spacing, text-transform and white-space on N rows in an overflow: auto panel with a tiled background image, plus list markers and a table.
tb-desc-selector-bombs = Extreme An+B offsets, a 10 000-character attribute value, 1 000 sibling rules, nested :not() and combinator characters inside quoted values: nothing panics and the quoted probes still match.
tb-desc-sibling-bombs = One container of N siblings under ~, :nth-child and :last-of-type rules while the .hot element moves every frame.
tb-desc-entity-bombs = 50 000 character references without a semicolon, a 100 000-letter name and references in attributes: decoding stays linear and correct.
tb-desc-function-bombs = 30 nested light-dark( and 30 nested calc() around a var(), in a stylesheet and in style attributes: the sheet is refused and the attributes dropped at once, not after minutes (bug_0179).
tb-desc-selector-engine = :not\0() (bug_0180), :has(), :nth-child(2 of .a), :nth-col(), nested rules, nested :not() and a far sibling combinator skip their rules while the sheet loads; :nth-child(0) colours nothing; :is(), :only-of-type and ^= match (ADR 0034).
tb-desc-selectors-vs-fresh = The selector list mutated at random every frame (change, insert, remove, move, highlight), compared with a fresh build of the same state every few frames.
tb-desc-restyle-vs-full = The focus moves to a random row every frame (:focus, :focus-visible, :focus-within, :not(), + and ~); every few frames the scoped result is compared with a forced full restyle of the same UI.
tb-desc-order-independence = Rows whose style depends only on themselves (data-* attributes, classes): a fresh build of the same rows in reverse order must style every row the same. Catches results cached on too little, which a same-order twin shares.
tb-desc-idle-quiet = calc() widths, an overflow: auto panel, a background image, transitions and structural selectors with identical contexts written every frame: after settling, no build, no restyle and no Node write.
tb-desc-scaling = A container of N siblings with unterminated references next to one of 4N; phases alternate which one churns, and the 4N frame may cost at most 8 times the N frame (best of three).

tb-size = Size: { $size }
tb-churn =
    { $on ->
        [true] Churn: on
       *[other] Churn: off
    }
tb-restart = Restart
tb-language = Language

tb-stat-frame = Frame { $avg } ms, max { $max } ms, { $fps } fps
tb-stat-updates = Builds { $builds }/s, restyles { $restyles }/s
tb-stat-size = UIs { $uis }, elements { $elements }

tb-check-budget =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } frame budget: worst { $detail } ms
tb-check-built =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } built { $detail }
tb-check-survived =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } survived { $detail }
tb-check-sheet-refused =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } stylesheet refused { $detail }
tb-check-no-forged =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } nothing forged { $detail }
tb-check-no-fallback =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } every payload localized { $detail }
tb-check-map-literal =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } map literal kept the value { $detail }
tb-check-include-refused =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } remote include refused { $detail }
tb-check-border-skipped =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } remote image skipped { $detail }
tb-check-twins =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } twins match { $detail }

tb-hint = N / P or LB / RB: scenario. − / + or [ / ] or LT / RT: size. C / X: churn. R / Select: restart. L / Y: language. Run with --features hotpath and `hotpath console` for live profiling.
tb-check-reuse =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } unchanged content reused { $detail }
tb-check-auto-scroll =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } overflow: auto scrolls exactly when needed { $detail }
tb-check-quiet =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } idle UI stays quiet { $detail }
tb-check-linear =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } 4N costs at most 8× N { $detail }
tb-check-quoted-match =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } quoted values match { $detail }
tb-check-decoded =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } references decode correctly { $detail }
tb-check-band =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } built rows cover the viewport { $detail }
tb-check-inline-dropped =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } the style attribute is dropped { $detail }
tb-check-engine-rules =
    { $state ->
        [pass] PASS
        [fail] FAIL
       *[other] WAIT
    } unsupported rules skip, :nth-child(0) matches nothing, Level 4 matches { $detail }
