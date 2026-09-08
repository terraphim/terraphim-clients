//! Knowledge-graph retrieval over the agent evolution memory store.
//!
//! `terraphim-agent memory retrieve` ranks captured memory items with the same
//! machinery the rest of Terraphim uses for document search: the role's
//! thesaurus drives an Aho-Corasick automaton, memory items are indexed as
//! [`Document`]s in a [`RoleGraph`], and ranking is the rolegraph's sum of node
//! rank, edge rank and document rank.
//!
//! Deliberately absent: any BM25 scorer, and any lexical scan over the whole
//! store. A query that touches none of the role's concepts returns no results
//! rather than falling back to substring matching -- see [`retrieve`].
//!
//! The graph built here is a scratch graph, private to one retrieval call. The
//! live per-role graphs owned by `ConfigState` index the role's haystacks and
//! must not have memory items inserted into them.

use std::collections::HashMap;

use terraphim_agent_evolution::MemoryItem;
use terraphim_rolegraph::RoleGraph;
use terraphim_types::{Document, RoleName, Thesaurus};

/// The result of one retrieval, including why it may be empty.
///
/// An empty `hits` has two distinct causes, and callers should not conflate
/// them: either the query touched none of the role's concepts
/// (`query_concepts` is empty), or it did touch concepts but no memory item is
/// indexed under them (`query_concepts` is non-empty). The first says the
/// question is outside the role's knowledge graph; the second says the store
/// has nothing to say about a question the role does understand.
#[derive(Debug, Clone)]
pub struct RetrievalOutcome {
    /// Concepts from the role's thesaurus that the query text itself matched.
    pub query_concepts: Vec<String>,
    /// Matching memory items, best-ranked first.
    pub hits: Vec<RetrievedMemory>,
}

/// A memory item that matched the query, with its knowledge-graph ranking.
#[derive(Debug, Clone)]
pub struct RetrievedMemory {
    /// The matched memory item.
    pub item: MemoryItem,
    /// Graph rank: the sum of node rank, edge rank and document rank.
    pub rank: u64,
    /// Concept(s) the rolegraph recorded for this match.
    ///
    /// The rolegraph records the concept of the *first* matching edge only;
    /// later edges aggregate into `rank` without extending this list. It is
    /// therefore a witness that the match was concept-driven, not a complete
    /// enumeration of every concept the item shares with the query.
    pub matched_concepts: Vec<String>,
}

/// Render a memory item as a [`Document`] for graph indexing.
///
/// Only `content` is indexed. `RoleGraph::insert_document` matches over the
/// document's `Display` form (title, body, description, summarization), which
/// excludes `tags` -- so tags travel with the result but do not themselves
/// create concept matches. That is how every other Terraphim document is
/// indexed, and memory items are not made a special case.
fn memory_to_document(item: &MemoryItem) -> Document {
    Document {
        id: item.id.clone(),
        url: format!("memory://{}", item.id),
        title: String::new(),
        body: item.content.clone(),
        tags: if item.tags.is_empty() {
            None
        } else {
            Some(item.tags.clone())
        },
        ..Default::default()
    }
}

/// Retrieve memory items matching `query`, ranked by the role's knowledge graph.
///
/// Returns no hits -- not an error -- when the query matches none of the
/// role's concepts. That is the intended outcome: retrieval is scoped to what
/// the role actually knows about, and a query outside the knowledge graph has
/// no concept-ranked answer to give. [`RetrievalOutcome::query_concepts`]
/// distinguishes that case from "the role knows these concepts, but no memory
/// item is indexed under them".
///
/// Two properties follow from the rolegraph's design and are worth knowing:
///
/// * An item is only reachable if it contains **at least two** concepts. The
///   graph is built from co-occurrence edges between consecutive matches, so a
///   single-concept item produces no edge and stays unindexed. This is
///   `RoleGraph::insert_document`'s behaviour for all documents, not something
///   introduced here.
/// * Ranking depends on the corpus. The same item scores differently as other
///   memory items are captured, because node and edge ranks aggregate across
///   every document indexed into the graph.
///
/// `offset` and `limit` are applied after a deterministic sort (rank
/// descending, then id ascending) so that paging is stable across calls.
/// `RoleGraph::query_graph` collects into a hash map and would otherwise return
/// equal-ranked items in arbitrary order.
pub fn retrieve(
    role: &RoleName,
    thesaurus: Thesaurus,
    items: &[MemoryItem],
    query: &str,
    offset: Option<usize>,
    limit: Option<usize>,
) -> anyhow::Result<RetrievalOutcome> {
    // Determine which of the role's concepts the query itself names, so an
    // empty result can say which of the two reasons applies. This runs the same
    // automaton the rolegraph builds, against the same thesaurus.
    let mut query_concepts: Vec<String> =
        terraphim_automata::find_matches(query, &thesaurus, false)
            .map_err(|e| anyhow::anyhow!("failed to match query against role thesaurus: {e}"))?
            .into_iter()
            .map(|m| m.normalized_term.display().to_string())
            .collect();
    query_concepts.sort();
    query_concepts.dedup();

    let mut graph = RoleGraph::new_sync(role.clone(), thesaurus)?;

    let mut by_id: HashMap<&str, &MemoryItem> = HashMap::with_capacity(items.len());
    for item in items {
        graph.insert_document(&item.id, memory_to_document(item));
        by_id.insert(item.id.as_str(), item);
    }

    // Page here rather than in `query_graph`, so the slice is taken from a
    // deterministic order rather than from hash-map iteration order.
    let mut ranked = graph.query_graph(query, None, None)?;
    ranked.sort_by(|(a_id, a), (b_id, b)| b.rank.cmp(&a.rank).then_with(|| a_id.cmp(b_id)));

    let hits = ranked
        .into_iter()
        .skip(offset.unwrap_or(0))
        .take(limit.unwrap_or(usize::MAX))
        .filter_map(|(id, doc)| {
            by_id.get(id.as_str()).map(|item| RetrievedMemory {
                item: (*item).clone(),
                rank: doc.rank,
                matched_concepts: doc.tags,
            })
        })
        .collect();

    Ok(RetrievalOutcome {
        query_concepts,
        hits,
    })
}

/// Collect every memory item in the store, across both retention buckets.
///
/// `MemoryState::add_memory` routes High and Critical importance items to
/// `long_term` and everything else to `short_term`, so reading only
/// `short_term` (as `memory list` does) hides precisely the items the store
/// considers most important. Retrieval reads both.
///
/// Episodic and semantic memory, and the lessons store, are out of scope here.
pub fn collect_memory_items(state: &terraphim_agent_evolution::MemoryState) -> Vec<MemoryItem> {
    let mut items: Vec<MemoryItem> = state.short_term.clone();
    items.extend(state.long_term.values().cloned());
    items
}

#[cfg(test)]
mod tests {
    use super::*;
    use terraphim_agent_evolution::{ImportanceLevel, MemoryItemType};
    use terraphim_types::{NormalizedTerm, NormalizedTermValue};

    /// Build a thesaurus mapping each synonym to its concept.
    ///
    /// `entries` is `(synonym, concept, concept_id)`. Several synonyms may share
    /// one concept, which is what makes graph retrieval more than substring
    /// matching.
    fn thesaurus(name: &str, entries: &[(&str, &str, u64)]) -> Thesaurus {
        let mut t = Thesaurus::new(name.to_string());
        for (synonym, concept, id) in entries {
            t.insert(
                NormalizedTermValue::from(*synonym),
                NormalizedTerm::new(*id, NormalizedTermValue::from(*concept)),
            );
        }
        t
    }

    fn memory(id: &str, content: &str) -> MemoryItem {
        MemoryItem {
            id: id.to_string(),
            item_type: MemoryItemType::Experience,
            content: content.to_string(),
            created_at: chrono::Utc::now(),
            last_accessed: None,
            access_count: 0,
            importance: ImportanceLevel::Medium,
            tags: Vec::new(),
            associations: std::collections::HashMap::new(),
        }
    }

    /// Two synonyms, one concept: the item says "bun", the query says
    /// "package manager", and no substring is shared. Retrieval succeeds only
    /// because both resolve to the same knowledge-graph concept.
    ///
    /// This is the property a BM25 or substring implementation cannot have, and
    /// the reason this command is built on the rolegraph.
    #[test]
    fn matches_through_a_shared_concept_not_a_shared_substring() {
        let t = thesaurus(
            "kg",
            &[
                ("bun", "bun", 1),
                ("package manager", "bun", 1),
                ("install", "install", 2),
            ],
        );
        let items = vec![memory("m1", "bun install is the sanctioned way")];

        let hits = retrieve(
            &RoleName::new("test-role"),
            t,
            &items,
            "package manager",
            None,
            None,
        )
        .expect("retrieval should succeed")
        .hits;

        assert_eq!(
            hits.iter().map(|h| h.item.id.as_str()).collect::<Vec<_>>(),
            vec!["m1"],
            "item sharing a concept but no substring with the query must be retrieved"
        );
        assert!(!hits[0].matched_concepts.is_empty());
    }

    /// A query touching no concept in the role's thesaurus returns nothing.
    /// This is intended: there is no lexical fallback to scan the store with.
    #[test]
    fn query_outside_the_knowledge_graph_returns_empty() {
        let t = thesaurus("kg", &[("bun", "bun", 1), ("install", "install", 2)]);
        let items = vec![memory("m1", "bun install is the sanctioned way")];

        let out = retrieve(
            &RoleName::new("test-role"),
            t,
            &items,
            "quarterly revenue forecast",
            None,
            None,
        )
        .expect("an unmatched query is not an error");

        assert!(
            out.query_concepts.is_empty(),
            "the query names none of the role's concepts"
        );
        assert!(
            out.hits.is_empty(),
            "no concept match must yield no results, not a lexical fallback"
        );
    }

    /// An item carrying only one concept produces no co-occurrence edge and so
    /// is not reachable. Pinned deliberately: it is `RoleGraph`'s behaviour for
    /// every document, and a reviewer seeing an empty result should be able to
    /// tell this apart from a bug.
    #[test]
    fn single_concept_item_is_not_indexed() {
        let t = thesaurus("kg", &[("bun", "bun", 1), ("install", "install", 2)]);
        let items = vec![memory("solo", "bun")];

        let out = retrieve(&RoleName::new("test-role"), t, &items, "bun", None, None)
            .expect("retrieval should succeed");

        assert_eq!(
            out.query_concepts,
            vec!["bun".to_string()],
            "the query itself does name a concept -- the item is simply unindexed"
        );
        assert!(
            out.hits.is_empty(),
            "a document with fewer than two concept matches creates no edge"
        );
    }

    /// More concept overlap with the query outranks less, and the order is
    /// stable across repeated calls.
    #[test]
    fn ranks_by_concept_overlap_and_is_deterministic() {
        let t = thesaurus(
            "kg",
            &[
                ("bun", "bun", 1),
                ("install", "install", 2),
                ("test", "test", 3),
            ],
        );
        let items = vec![
            memory("rich", "bun install and bun test both work"),
            memory("thin", "bun install"),
        ];

        let first = retrieve(
            &RoleName::new("test-role"),
            t.clone(),
            &items,
            "bun install test",
            None,
            None,
        )
        .expect("retrieval should succeed")
        .hits;

        let ids: Vec<&str> = first.iter().map(|h| h.item.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["rich", "thin"],
            "the item overlapping more concepts must rank first"
        );

        for _ in 0..8 {
            let again = retrieve(
                &RoleName::new("test-role"),
                t.clone(),
                &items,
                "bun install test",
                None,
                None,
            )
            .expect("retrieval should succeed")
            .hits;
            assert_eq!(
                again.iter().map(|h| h.item.id.as_str()).collect::<Vec<_>>(),
                ids,
                "ordering must not depend on hash iteration order"
            );
        }
    }

    /// `limit` and `offset` page the deterministic ordering.
    #[test]
    fn limit_and_offset_page_the_ranked_order() {
        let t = thesaurus(
            "kg",
            &[
                ("bun", "bun", 1),
                ("install", "install", 2),
                ("test", "test", 3),
            ],
        );
        let items = vec![
            memory("rich", "bun install and bun test both work"),
            memory("thin", "bun install"),
        ];
        let role = RoleName::new("test-role");

        let page = retrieve(&role, t.clone(), &items, "bun install test", None, Some(1))
            .expect("retrieval should succeed")
            .hits;
        assert_eq!(
            page.iter().map(|h| h.item.id.as_str()).collect::<Vec<_>>(),
            vec!["rich"]
        );

        let page = retrieve(&role, t, &items, "bun install test", Some(1), Some(1))
            .expect("retrieval should succeed")
            .hits;
        assert_eq!(
            page.iter().map(|h| h.item.id.as_str()).collect::<Vec<_>>(),
            vec!["thin"]
        );
    }

    /// High-importance items live in `long_term`, not `short_term`. Retrieval
    /// must see both buckets.
    #[test]
    fn collect_reads_both_retention_buckets() {
        let mut state = terraphim_agent_evolution::MemoryState::default();

        let mut low = memory("low", "bun install");
        low.importance = ImportanceLevel::Medium;
        let mut high = memory("high", "bun test");
        high.importance = ImportanceLevel::Critical;

        state.add_memory(low);
        state.add_memory(high);

        assert_eq!(state.short_term.len(), 1, "medium importance -> short_term");
        assert_eq!(state.long_term.len(), 1, "critical importance -> long_term");

        let mut ids: Vec<String> = collect_memory_items(&state)
            .into_iter()
            .map(|m| m.id)
            .collect();
        ids.sort();
        assert_eq!(ids, vec!["high".to_string(), "low".to_string()]);
    }
}
