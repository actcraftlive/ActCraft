# Translating ActCraft

English is ActCraft's built-in language and the default. Every other language is one language
file in `crates/actcraft-ui/i18n/`, named by its language code: `de.tsv` (German), `fr.tsv`
(French), `pt-br.tsv` (Brazilian Portuguese). The build picks up every file there; nothing else
needs to change. The language then appears in Settings > Language under its own name.

## The file format

A language file is plain UTF-8 text in the [craft-i18n catalog format](https://github.com/benandez/craft-i18n):
one translation per line, three columns separated by a tab:

```text
<context>	<English text>	<translation>
```

- The context is usually empty, so a line starts with a tab.
- Lines starting with `#` are comments.
- The line `@id	language.name	<name>` gives the language's name in itself (`Deutsch`,
  `Français`). Every file needs it.
- `{name}` placeholders and a trailing `…` must stay as they are in the English text.
- Write `\n` for a line break, `\t` for a tab and `\\` for a backslash.

Example from `de.tsv`:

```text
@id	language.name	Deutsch
	Play	Abspielen
	Add songs…	Songs hinzufügen…
```

## Adding a language

1. Copy `de.tsv` to `<code>.tsv` and change the `language.name` row.
2. Replace the third column of every line with your translation.
3. Run `cargo test -p actcraft-ui`. The test `every_window_text_is_in_every_language_file`
   lists every window text your file doesn't have yet; `language_files_parse_cleanly` names
   lines that break the format.
4. Open a pull request.

A text missing from a file shows in English, so a running app never breaks; the test makes
sure a file that ships is complete.

## Adding window text

Every visible text goes through `tr("English text")` in `crates/actcraft-ui`. When you add one,
add its row to every language file; the test above fails until you do.
