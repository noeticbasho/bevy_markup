tb-title = Prüfstand
tb-group-perf = Leistung
tb-group-security = Sicherheit
tb-group-correctness = Korrektheit

tb-sc-big-document = Großes Dokument
tb-sc-all-dirty = Alles geändert
tb-sc-restyle-storm = Restyle-Sturm
tb-sc-markdown = Eingerücktes Markdown
tb-sc-localized = Lokalisierte Zeilen
tb-sc-many-uis = Viele UIs
tb-sc-calc-layout = calc()-Layout
tb-sc-nested-rules = Verschachtelte CSS-Regeln
tb-sc-deep-dom = Tiefes DOM
tb-sc-var-bombs = var()-Bomben
tb-sc-grid-bombs = Grid-Bomben
tb-sc-hostile-args = Feindliche Fluent-Argumente
tb-sc-args-injection = Argument-JSON-Injektion
tb-sc-remote-sources = Entfernte Quellen
tb-sc-markdown-bombs = Markdown-Bomben
tb-sc-reuse-vs-fresh = Wiederverwendung gegen Neubau
tb-sc-indent-vs-flat = Eingerückt gegen flach
tb-sc-selector-list = Selektorliste
tb-sc-text-features = Texteigenschaften
tb-sc-selector-bombs = Selektor-Bomben
tb-sc-sibling-bombs = Geschwister-Bomben
tb-sc-entity-bombs = Entity-Bomben
tb-sc-function-bombs = Funktions-Bomben
tb-sc-selector-engine = Selektor-Engine
tb-sc-selectors-vs-fresh = Selektoren gegen Neubau
tb-sc-restyle-vs-full = Gezielt gegen vollständig neu gestaltet
tb-sc-order-independence = Reihenfolge-Unabhängigkeit
tb-sc-idle-quiet = Leerlauf bleibt ruhig
tb-sc-scaling = Skalierung N gegen 4N

tb-desc-big-document = Ein Dokument aus N Zeilen; pro Frame ändert sich etwa eine von hundert, unveränderte Zeilen verwenden den vorigen Aufbau wieder.
tb-desc-all-dirty = Jede Zeile ändert sich in jedem Frame: Rendern, Parsen, Kaskade und Abgleich vollständig.
tb-desc-restyle-storm = Eine Kontext-var() färbt jede Zeile neu, und eine Hervorhebung wandert pro Frame: Neugestaltung an Ort und Stelle.
tb-desc-markdown = Eine Markdown-Vorlage mit eingerückten Tera-Blöcken, in jedem Frame neu gerendert.
tb-desc-localized = N Zeilen Fluent-Nachrichten in einem Markdown-Bündel, gecachter und Pro-Wert-Pfad, Argumente ändern sich pro Frame.
tb-desc-many-uis = N getrennte kleine UIs; pro Frame wird eine davon aktualisiert.
tb-desc-calc-layout = N Karten mit calc() aus gemischten Einheiten, pro Frame gegen die Bühne aufgelöst.
tb-desc-nested-rules = Ein Stylesheet aus 20 000 verschachtelten Regeln: das Laden muss sauber scheitern, statt die App abzubrechen.
tb-desc-deep-dom = 5 000 verschachtelte Elemente: Inhalt jenseits von 100 Ebenen entfällt, der Rest wird aufgebaut.
tb-desc-var-bombs = var()-Ketten, sich verdoppelnde Ersatzwerte, Zyklen und verschachtelte Funktionen: die Ersetzung bleibt begrenzt.
tb-desc-grid-bombs = Grid-Spurzahlen, die früher überliefen: die Deklarationen entfallen, nichts stürzt ab.
tb-desc-hostile-args = Fluent-Textargumente mit HTML, Markdown-Links und -Bildern und präparierten Markern: als Text angezeigt, nichts gefälscht.
tb-desc-args-injection = Argumente aus einem feindlichen Namen auf zwei Arten gebaut: das Tera-Map-Literal behält ihn ganz, das handgeschriebene JSON ist injizierbar.
tb-desc-remote-sources = Ein https://-Include und border-image: das Include scheitert beim Laden, das Bild wird übersprungen.
tb-desc-markdown-bombs = Pathologisches Markdown: tiefe Hervorhebung, Klammern und Zitate, präparierte Klassenmarker, verschachtelte eingerückte Tera-Blöcke.
tb-desc-reuse-vs-fresh = Eine UI, die in jedem Frame zufällig verändert wird, wird alle paar Frames mit einem frischen Aufbau desselben Zustands verglichen.
tb-desc-indent-vs-flat = Eine eingerückte und eine flache Markdown-Vorlage desselben Dokuments müssen dieselbe UI ergeben.
tb-desc-selector-list = N Zeilen in einem Container, dazu eine Liste und eine Tabelle, gestaltet mit Struktur-Pseudoklassen, Geschwister-Kombinatoren, Attributselektoren und :not(); pro Frame ändert sich etwa eine von hundert Zeilen, die unveränderten müssen weiter wiederverwendet werden.
tb-desc-text-features = text-align, line-height, letter-spacing, text-transform und white-space auf N Zeilen in einem overflow: auto-Feld mit gekacheltem Hintergrundbild, dazu Listenmarken und eine Tabelle.
tb-desc-selector-bombs = Extreme An+B-Werte, ein Attributwert mit 10 000 Zeichen, 1 000 Geschwisterregeln, verschachtelte :not() und Kombinator-Zeichen in Werten in Anführungszeichen: nichts stürzt ab, und die Prüf-Elemente passen weiterhin.
tb-desc-sibling-bombs = Ein Container mit N Geschwistern unter Regeln mit ~, :nth-child und :last-of-type, während das .hot-Element in jedem Frame wandert.
tb-desc-entity-bombs = 50 000 Zeichenreferenzen ohne Semikolon, ein Name aus 100 000 Buchstaben und Referenzen in Attributen: die Dekodierung bleibt linear und korrekt.
tb-desc-function-bombs = 30 verschachtelte light-dark( und 30 verschachtelte calc() um ein var(), im Stylesheet und in style-Attributen: das Stylesheet wird sofort abgelehnt und die Attribute verworfen, nicht erst nach Minuten (bug_0179).
tb-desc-selector-engine = :not\0() (bug_0180), :has(), :nth-child(2 of .a), :nth-col(), verschachtelte Regeln, verschachteltes :not() und ein entfernter Geschwister-Kombinator überspringen ihre Regeln, während das Stylesheet lädt; :nth-child(0) färbt nichts; :is(), :only-of-type und ^= passen (ADR 0034).
tb-desc-selectors-vs-fresh = Die Selektorliste wird in jedem Frame zufällig verändert (ändern, einfügen, entfernen, verschieben, hervorheben) und alle paar Frames mit einem Neubau desselben Zustands verglichen.
tb-desc-restyle-vs-full = Der Fokus springt in jedem Frame zu einer zufälligen Zeile (:focus, :focus-visible, :focus-within, :not(), + und ~); alle paar Frames wird das gezielte Ergebnis mit einer erzwungenen vollständigen Neugestaltung derselben UI verglichen.
tb-desc-order-independence = Zeilen, deren Stil nur von ihnen selbst abhängt (data-*-Attribute, Klassen): ein Neubau derselben Zeilen in umgekehrter Reihenfolge muss jede Zeile gleich gestalten. Findet zu knapp zwischengespeicherte Ergebnisse, die ein Zwilling in gleicher Reihenfolge teilt.
tb-desc-idle-quiet = calc()-Breiten, ein overflow: auto-Feld, ein Hintergrundbild, Übergänge und Struktur-Selektoren, in jedem Frame mit identischem Kontext beschrieben: nach dem Einschwingen kein Aufbau, keine Neugestaltung und kein Node-Schreiben.
tb-desc-scaling = Ein Container mit N Geschwistern und unterminierten Referenzen neben einem mit 4N; Phasen wechseln, welcher sich ändert, und der 4N-Frame darf höchstens 8-mal so viel kosten wie der N-Frame (bester von drei).

tb-size = Größe: { $size }
tb-churn =
    { $on ->
        [true] Änderungen: an
       *[other] Änderungen: aus
    }
tb-restart = Neustart
tb-language = Sprache

tb-stat-frame = Frame { $avg } ms, max. { $max } ms, { $fps } fps
tb-stat-updates = Aufbauten { $builds }/s, Neugestaltungen { $restyles }/s
tb-stat-size = UIs { $uis }, Elemente { $elements }

tb-check-budget =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } Frame-Budget: schlechtester { $detail } ms
tb-check-built =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } aufgebaut { $detail }
tb-check-survived =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } überlebt { $detail }
tb-check-sheet-refused =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } Stylesheet abgelehnt { $detail }
tb-check-no-forged =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } nichts gefälscht { $detail }
tb-check-no-fallback =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } jede Nutzlast lokalisiert { $detail }
tb-check-map-literal =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } Map-Literal behielt den Wert { $detail }
tb-check-include-refused =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } entferntes Include abgelehnt { $detail }
tb-check-border-skipped =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } entferntes Bild übersprungen { $detail }
tb-check-twins =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } Zwillinge gleich { $detail }

tb-hint = N / P oder LB / RB: Szenario. − / + oder [ / ] oder LT / RT: Größe. C / X: Änderungen. R / Select: Neustart. L / Y: Sprache. Mit --features hotpath und `hotpath console` live profilieren.
tb-check-reuse =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } unveränderter Inhalt wiederverwendet { $detail }
tb-check-auto-scroll =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } overflow: auto scrollt genau bei Bedarf { $detail }
tb-check-quiet =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } ruhende UI bleibt ruhig { $detail }
tb-check-linear =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } 4N kostet höchstens das 8-Fache von N { $detail }
tb-check-quoted-match =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } Werte in Anführungszeichen passen { $detail }
tb-check-decoded =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } Referenzen korrekt dekodiert { $detail }
tb-check-band =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } gebaute Zeilen decken den Sichtbereich { $detail }
tb-check-inline-dropped =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } das style-Attribut wird verworfen { $detail }
tb-check-engine-rules =
    { $state ->
        [pass] OK
        [fail] FEHLER
       *[other] WARTE
    } nicht unterstützte Regeln entfallen, :nth-child(0) passt auf nichts, Level 4 passt { $detail }
