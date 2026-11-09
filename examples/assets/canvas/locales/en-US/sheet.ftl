sheet-title = Character sheet
sheet-drag = Drag to turn the hero, click to change the armor.
sheet-name = Name: <b>{ $name }</b>
sheet-armor = Armor: <b>{ $armor ->
        [leather] leather
        [steel] steel
       *[gold] gold
    }</b>
sheet-thumb = The same camera, shown as an image.
sheet-armor-button = Change armor
sheet-toggle = { $shown ->
        [true] Hide the portrait
       *[other] Show the portrait
    }
sheet-language = Language: English
sheet-status = { $rendering ->
        [true] The portrait camera renders.
       *[other] The portrait camera is off: no canvas shows it.
    }
sheet-hint = Drag or click the portrait. Q / E or LB / RB turn the hero. ↑ ↓ / D-pad and Enter / A press the buttons. Space / Y switches the language.
