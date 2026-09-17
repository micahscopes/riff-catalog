use riff_catalog_solidity::{
    selection::{SourceArtifact, View},
    similarity::{IndexBudget, SyntaxIndex, tokens},
};
use serde_json::json;

#[test]
fn coverage_lexer_ignores_comments_but_not_strings() {
    let source = "x += 1; /* no tokens */ f(\"//yes\\\"ok\"); // tail\n ü++";
    let slices: Vec<_> = tokens(source, 0)
        .iter()
        .map(|s| source[s.start..s.start + s.length].to_string())
        .collect();
    assert_eq!(
        slices,
        [
            "x",
            "+=",
            "1",
            ";",
            "f",
            "(",
            "\"//yes\\\"ok\"",
            ")",
            ";",
            "ü",
            "++"
        ]
    );
}

#[test]
fn index_exhaustion_is_explicit() {
    let artifacts = [SourceArtifact {
        path: "a.sol".into(),
        source: "x".into(),
        ast: json!({"id":1,"nodeType":"Identifier","src":"0:1:0","name":"x"}),
    }];
    for budget in [
        IndexBudget {
            occurrences: 0,
            ..Default::default()
        },
        IndexBudget {
            normalized_nodes: 0,
            ..Default::default()
        },
        IndexBudget {
            serialized_bytes: 0,
            ..Default::default()
        },
    ] {
        let index = SyntaxIndex::build(&artifacts, View::Names, budget).unwrap();
        assert!(index.truncated);
        assert!(index.overlap(0, 0, 1, 1, 10).unwrap().truncated);
    }
}

#[test]
#[ignore = "requires local solc; run explicitly with RIFFCAT_SOLC"]
fn real_source_partial_overlap_is_one_to_one_not_whole_equivalence() {
    let sources = [
        "contract C { function f(uint x) public pure returns(uint) { x=x+1; x=x+1; return x; } }",
        "contract R { function g(uint a) public pure returns(uint) { a=a+1; a=a+1; return a; } }",
        "contract D { function h(uint b) public pure returns(uint) { b=b+1; return b; } }",
    ];
    let runner = riff_catalog_solc::SolcRunner::locate(None);
    let artifacts:Vec<_>=sources.iter().enumerate().map(|(i,s)| {
        let output=runner.compile(&json!({"language":"Solidity","sources":{"input.sol":{"content":s}},"settings":{"outputSelection":{"*":{"": ["ast"]}}}})).unwrap();
        output.check_errors().unwrap();
        SourceArtifact {path:format!("{i}.sol"),source:s.to_string(),ast:output.source_ast("input.sol").unwrap().clone()}
    }).collect();
    let index = SyntaxIndex::build(&artifacts, View::Bindings, IndexBudget::default()).unwrap();
    let report = index
        .overlap(
            0,
            sources[0].find("function").unwrap(),
            sources[0].rfind('}').unwrap() - 1,
            3,
            200,
        )
        .unwrap();
    assert!(!report.truncated);
    assert!(report.unsupported.is_empty(), "{:?}", report.unsupported);
    assert_eq!(report.results.len(), 2);
    let exact = &report.results[0];
    assert!(exact.whole_match);
    assert_eq!(exact.file, 1);
    assert_eq!(exact.query_covered_tokens, exact.query_total_tokens);
    assert_eq!(exact.fragments.len(), 1); // no nested double-counting
    let partial = &report.results[1];
    assert!(!partial.whole_match);
    assert_eq!(partial.file, 2);
    assert!(
        partial.query_covered_tokens > 0
            && partial.query_covered_tokens < partial.query_total_tokens
    );
    assert_eq!(
        partial
            .fragments
            .iter()
            .map(|f| f.query_tokens)
            .sum::<usize>(),
        partial.query_covered_tokens
    );
    assert_eq!(
        partial
            .fragments
            .iter()
            .map(|f| f.candidate_tokens)
            .sum::<usize>(),
        partial.candidate_covered_tokens
    );
    assert_eq!(
        partial.query_unmatched.len() + partial.query_covered_tokens,
        partial.query_total_tokens
    );
    for (i, a) in partial.fragments.iter().enumerate() {
        for b in &partial.fragments[i + 1..] {
            assert!(
                a.query.start + a.query.length <= b.query.start
                    || b.query.start + b.query.length <= a.query.start
            );
            assert!(
                a.candidate.start + a.candidate.length <= b.candidate.start
                    || b.candidate.start + b.candidate.length <= a.candidate.start
            );
        }
    }
}
