# trash

Move files to the trash instead of deleting them irrecoverably. This concept
exists so `tests/hook_safety.rs` can exercise the PreToolUse replacement path
(Refs #126) against a fixture thesaurus rather than whatever knowledge graph
happens to be installed on the developer's machine.

synonyms:: rm -rf, rm -r
