# Save a lesson and recover it in another process

`memory capture` requires the actual lesson, either as `--content` or a UTF-8
`--file`. A provenance tag identifies the source; it is not a substitute for
the lesson. Blank, unreadable and conflicting inputs fail without writing a
memory. Content is preserved exactly, up to the evolution crate's 1 MB limit.

```sh
export TERRAPHIM_DEFAULT_DATA_PATH="$PWD/.terraphim-demo-data"
terraphim-agent --format json memory capture \
  --content 'Validate the garden brief with python3 validate.py --brief brief.md.' \
  --provenance-tag public-brief-v1
terraphim-agent --format json memory list
terraphim-agent --format json memory show MEMORY_ID_FROM_CAPTURE
terraphim-agent memory export --format json --output memories.json
```

These commands each start a new process. With the explicit data root, the
evolution store is `.terraphim-demo-data/terraphim/evolution/cli-agent.json`.
Without it, the historical platform configuration-directory path is retained;
existing stores are not moved or imported. Use a different data root for a
separate project store. This is file-location isolation, not an access-control
system or automatic secret redaction.

To save a multiline note, use `memory capture --file lesson.md`. Text and file
input are mutually exclusive. Earlier versions accepted capture with only a
provenance tag and saved placeholder text; callers must now supply content.

Retrieval is a separate operation using the selected role's knowledge graph:

```sh
terraphim-agent --config role-config.json memory retrieve 'validate brief' \
  --role 'Brief Reviewer' --format json
```

Configure that role and its graph before retrieval. Saving a lesson does not
create terminology or copy it into the separate `learn` store. Graph retrieval
requires concept co-occurrence in the lesson; a role that does not recognise
the query returns no memories rather than a lexical fallback. A role is a
retrieval configuration, not a confidentiality boundary. `memory apply` is a
preview of proposed context and does not execute commands.
