# Languages

The interface speaks English, Slovak and Czech: choose it with the *Language* button in the main menu or in
the menu's *Gameplay* tab. Menus, the sidebar, notifications, the tutorial, the end screen and unit and
structure names and descriptions, and the campaign missions' names and briefings are translated; names and
briefings written for custom levels show as they were typed. Translations live
in `assets/i18n/sk.json` and `assets/i18n/cs.json`, each mapping the English text to its translation; they are
built into the game, so a change needs a new build. A text missing there simply stays English, and the tests
check that both files list the same texts with the same `{}` placeholders.
`tools/add_translations.py add "English" "Slovak" "Czech"` (or `remove "English"`) edits both files at once.
