use super::*;

/// The `initialize` response of `Microsoft.CodeAnalysis.LanguageServer` 2.140.9,
/// captured verbatim from the real server over `--stdio`.
///
/// Not a hand-written fixture on purpose: every shape this module has to be
/// permissive about — a provider advertised as an options object, a
/// `textDocumentSync` object, an absent `positionEncoding`, and a dozen
/// capabilities we do not model at all — is here because a real server sent it.
/// A test over invented JSON would only prove that the decoder agrees with the
/// author's reading of the specification.
// `r##` rather than `r#`: the captured payload contains the JSON string `"#"`
// among Roslyn's commit characters, and `"#` closes an `r#` literal.
const CAPTURED_INITIALIZE_RESULT: &str = r##"{
  "_roslyn_processId": 21732,
  "capabilities": {
    "_vs_onAutoInsertProvider": { "_vs_triggerCharacters": ["'", "/", "\n", "\""] },
    "textDocumentSync": { "openClose": true, "change": 2, "save": {} },
    "completionProvider": {
      "triggerCharacters": ["\"", "(", ":", "[", "\\", "{", "#", ".", ">", " ", "~", "<"],
      "allCommitCharacters": [" ", "{", "}", "[", "]", "(", ")", ".", ",", ":", ";", "+", "-", "*", "/", "%", "&", "|", "^", "!", "~", "=", "<", ">", "?", "@", "#", "'", "\"", "\\"],
      "resolveProvider": true
    },
    "hoverProvider": true,
    "signatureHelpProvider": {
      "triggerCharacters": ["(", ",", "[", "<", "{"],
      "retriggerCharacters": [")", "]", ">", "}"]
    },
    "definitionProvider": true,
    "typeDefinitionProvider": true,
    "implementationProvider": true,
    "referencesProvider": { "workDoneProgress": true },
    "documentHighlightProvider": true,
    "documentSymbolProvider": true,
    "codeActionProvider": {
      "codeActionKinds": ["quickfix", "refactor"],
      "resolveProvider": true
    },
    "codeLensProvider": { "resolveProvider": true },
    "documentFormattingProvider": true,
    "documentRangeFormattingProvider": true,
    "documentOnTypeFormattingProvider": {
      "firstTriggerCharacter": "}",
      "moreTriggerCharacter": [";", "\n"]
    },
    "renameProvider": { "prepareProvider": true },
    "foldingRangeProvider": true,
    "executeCommandProvider": { "commands": [] },
    "selectionRangeProvider": true,
    "callHierarchyProvider": true,
    "semanticTokensProvider": {
      "legend": { "tokenTypes": ["namespace", "type", "class"], "tokenModifiers": ["static"] },
      "range": true,
      "full": true
    },
    "typeHierarchyProvider": true,
    "inlayHintProvider": { "resolveProvider": true },
    "workspaceSymbolProvider": true,
    "workspace": {}
  }
}"##;

/// The `textDocument/documentSymbol` answer for `sidecar/inspector/Collections.cs`,
/// captured verbatim from the same server.
///
/// Two things in here are the reason this fixture is real rather than invented:
/// the non-standard `glyph` field on every node (which must be ignored, not
/// rejected), and `name` carrying a whole signature.
const CAPTURED_DOCUMENT_SYMBOLS: &str = r#"[
  {
    "glyph": 48,
    "children": [
      {
        "glyph": 7,
        "children": [
          {
            "glyph": 49,
            "children": [],
            "name": "TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool",
            "detail": "TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool",
            "kind": 6,
            "range": { "start": { "line": 25, "character": 4 }, "end": { "line": 47, "character": 5 } },
            "selectionRange": { "start": { "line": 25, "character": 23 }, "end": { "line": 25, "character": 37 } }
          },
          {
            "glyph": 51,
            "children": [],
            "name": "TryReadBackingArray(ClrObject, IReadOnlyList<ClrObject>, int) : bool",
            "detail": "TryReadBackingArray(ClrObject, IReadOnlyList<ClrObject>, int) : bool",
            "kind": 6,
            "range": { "start": { "line": 56, "character": 4 }, "end": { "line": 105, "character": 5 } },
            "selectionRange": { "start": { "line": 56, "character": 24 }, "end": { "line": 56, "character": 43 } }
          }
        ],
        "name": "Collections",
        "detail": "Collections",
        "kind": 5,
        "range": { "start": { "line": 18, "character": 0 }, "end": { "line": 106, "character": 1 } },
        "selectionRange": { "start": { "line": 18, "character": 22 }, "end": { "line": 18, "character": 33 } }
      }
    ],
    "name": "CodeBasics.Inspector",
    "detail": "CodeBasics.Inspector",
    "kind": 3,
    "range": { "start": { "line": 2, "character": 0 }, "end": { "line": 106, "character": 1 } },
    "selectionRange": { "start": { "line": 2, "character": 10 }, "end": { "line": 2, "character": 20 } }
  }
]"#;

/// The `textDocument/definition` answer, captured verbatim.
///
/// The client advertised `linkSupport: true` and the server answered with a
/// plain `Location[]` anyway — which is legal, and is why the decoder cannot
/// assume the shape it asked for.
const CAPTURED_DEFINITION: &str = r#"[
  {
    "uri": "file:///C:/Users/AnthonyJames/Documents/Code/code-basics/sidecar/inspector/Collections.cs",
    "range": { "start": { "line": 25, "character": 23 }, "end": { "line": 25, "character": 37 } }
  }
]"#;

fn json(text: &str) -> Value {
    serde_json::from_str(text).expect("the captured payload is valid JSON")
}

fn position(line: u32, character: u32) -> Position {
    Position { line, character }
}

// ---------------------------------------------------------------- method names

#[test]
fn every_method_constant_is_spelled_the_way_the_protocol_spells_it() {
    // A method name is the one string in this subsystem that cannot be checked
    // by the type system and fails silently when wrong: the server answers
    // `-32601` and the feature simply reports nothing. Pinning them here means
    // a typo is a test failure rather than a missing usage list.
    assert_eq!("initialize", method::INITIALIZE);
    assert_eq!("initialized", method::INITIALIZED);
    assert_eq!("shutdown", method::SHUTDOWN);
    assert_eq!("exit", method::EXIT);
    assert_eq!("textDocument/didOpen", method::DID_OPEN);
    assert_eq!("textDocument/didChange", method::DID_CHANGE);
    assert_eq!("textDocument/didClose", method::DID_CLOSE);
    assert_eq!("textDocument/references", method::REFERENCES);
    assert_eq!("textDocument/definition", method::DEFINITION);
    assert_eq!("textDocument/implementation", method::IMPLEMENTATION);
    assert_eq!("textDocument/typeDefinition", method::TYPE_DEFINITION);
    assert_eq!("textDocument/documentSymbol", method::DOCUMENT_SYMBOL);
    assert_eq!("textDocument/rename", method::RENAME);
    assert_eq!("textDocument/prepareRename", method::PREPARE_RENAME);
    assert_eq!("$/cancelRequest", method::CANCEL_REQUEST);
    assert_eq!("workspace/configuration", method::CONFIGURATION);
    assert_eq!("client/registerCapability", method::REGISTER_CAPABILITY);
    assert_eq!("client/unregisterCapability", method::UNREGISTER_CAPABILITY);
    assert_eq!("window/showMessageRequest", method::SHOW_MESSAGE_REQUEST);
    assert_eq!(
        "window/workDoneProgress/create",
        method::WORK_DONE_PROGRESS_CREATE
    );
    assert_eq!("workspace/applyEdit", method::APPLY_EDIT);
    assert_eq!("window/logMessage", method::LOG_MESSAGE);
    assert_eq!("$/progress", method::PROGRESS);
}

#[test]
fn the_two_server_requests_the_real_server_sends_have_constants() {
    // Captured from the real start-up: these are the two that hang the server
    // when unanswered, so the transport looks them up by constant.
    let observed = ["window/workDoneProgress/create", "workspace/configuration"];
    assert!(observed.contains(&method::WORK_DONE_PROGRESS_CREATE));
    assert!(observed.contains(&method::CONFIGURATION));
}

// ------------------------------------------------------------------ decode_goto

#[test]
fn a_goto_response_of_null_decodes_to_no_locations() {
    assert_eq!(
        Vec::<Location>::new(),
        decode_goto(Value::Null).expect("null is legal")
    );
}

#[test]
fn the_captured_empty_type_definition_array_decodes_to_no_locations() {
    // The real server answered `[]`, not `null`. Both mean "none", and a
    // decoder that only knew one of them would report a protocol failure for
    // the ordinary case of a type with no separate declaration.
    let empty = json("[]");
    assert_eq!(
        Vec::<Location>::new(),
        decode_goto(empty).expect("[] is legal")
    );
}

#[test]
fn the_captured_definition_location_array_decodes_to_its_one_location() {
    let locations = decode_goto(json(CAPTURED_DEFINITION)).expect("a captured payload");
    assert_eq!(1, locations.len());
    assert_eq!(
        "file:///C:/Users/AnthonyJames/Documents/Code/code-basics/sidecar/inspector/Collections.cs",
        locations[0].uri
    );
    assert_eq!(position(25, 23), locations[0].range.start);
    assert_eq!(position(25, 37), locations[0].range.end);
}

#[test]
fn a_goto_response_of_one_bare_location_object_decodes_to_one_location() {
    let single = json(
        r#"{"uri":"file:///c:/x.rs","range":{"start":{"line":1,"character":2},"end":{"line":1,"character":5}}}"#,
    );
    let locations = decode_goto(single).expect("a bare Location is legal");
    assert_eq!(1, locations.len());
    assert_eq!("file:///c:/x.rs", locations[0].uri);
    assert_eq!(position(1, 2), locations[0].range.start);
}

#[test]
fn a_location_link_is_aimed_at_its_selection_range_and_not_its_whole_body() {
    // `targetRange` is the whole declaration, so its start is the `pub`/`class`
    // keyword or an attribute line; `targetSelectionRange` is the identifier.
    // Jumping to the former puts the cursor on a brace or a doc comment, which
    // is the difference between "navigate to the symbol" and "navigate near it".
    let links = json(
        r#"[{
            "targetUri": "file:///c:/x.rs",
            "targetRange": {"start":{"line":10,"character":0},"end":{"line":20,"character":1}},
            "targetSelectionRange": {"start":{"line":10,"character":11},"end":{"line":10,"character":14}}
        }]"#,
    );
    let locations = decode_goto(links).expect("LocationLink[] is legal");
    assert_eq!(1, locations.len());
    assert_eq!("file:///c:/x.rs", locations[0].uri);
    assert_eq!(position(10, 11), locations[0].range.start);
    assert_eq!(position(10, 14), locations[0].range.end);
}

#[test]
fn the_location_link_rust_analyzer_really_sends_is_aimed_at_its_selection_range() {
    // **Captured, not hand-written.** Every other `LocationLink` case in this
    // file is JSON somebody typed, which proves the decoder handles the shape in
    // the specification and proves nothing about the shape a server sends. Roslyn
    // answers `Location[]` even though we ask with `linkSupport: true`, so for a
    // long time the `Links` arm — and `aim`'s whole reason for existing — was
    // backed by no traffic at all.
    //
    // This is the verbatim `textDocument/definition` result from rust-analyzer
    // 1.97.1, asked at `crate::try_get_elements(source)` in the Rust oracle's own
    // fixture (`tests/lsp_oracle.rs::write_rust`), with only the temporary
    // `targetUri` shortened. So: `linkSupport: true` does get honoured by some
    // server, `originSelectionRange` is sent and ignored, and — the point of the
    // test — `targetRange` starts on the **doc comment** at line 2 while
    // `targetSelectionRange` starts on the identifier at line 3. Aiming at
    // `targetRange` would land the cursor on `/// Declared here…`.
    let links = json(
        r#"[{
            "originSelectionRange": {"start":{"line":1,"character":17},"end":{"line":1,"character":33}},
            "targetUri": "file:///c:/oracle/lib.rs",
            "targetRange": {"start":{"line":2,"character":0},"end":{"line":9,"character":1}},
            "targetSelectionRange": {"start":{"line":3,"character":7},"end":{"line":3,"character":23}}
        }]"#,
    );
    let locations = decode_goto(links).expect("rust-analyzer sends LocationLink[]");
    assert_eq!(1, locations.len());
    assert_eq!("file:///c:/oracle/lib.rs", locations[0].uri);
    assert_eq!(position(3, 7), locations[0].range.start);
    assert_eq!(position(3, 23), locations[0].range.end);
}

#[test]
fn a_location_link_without_a_selection_range_falls_back_to_its_target_range() {
    // `targetSelectionRange` is required by the specification, and a server that
    // omits it still gave us a usable file and a usable line. The whole range is
    // a worse anchor than the identifier and a much better answer than none.
    let links = json(
        r#"[{
            "targetUri": "file:///c:/x.rs",
            "targetRange": {"start":{"line":3,"character":0},"end":{"line":9,"character":1}}
        }]"#,
    );
    let locations = decode_goto(links).expect("a permissive decode");
    assert_eq!(position(3, 0), locations[0].range.start);
    assert_eq!(position(9, 1), locations[0].range.end);
}

#[test]
fn a_goto_response_of_several_locations_keeps_them_all_in_order() {
    let many = json(
        r#"[
            {"uri":"file:///c:/a.rs","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}},
            {"uri":"file:///c:/b.rs","range":{"start":{"line":5,"character":0},"end":{"line":5,"character":1}}}
        ]"#,
    );
    let locations = decode_goto(many).expect("Location[] is legal");
    assert_eq!(2, locations.len());
    assert_eq!("file:///c:/a.rs", locations[0].uri);
    assert_eq!("file:///c:/b.rs", locations[1].uri);
}

#[test]
fn a_goto_response_of_no_legal_shape_is_an_error_and_not_an_empty_list() {
    // The whole point of the error: an empty answer and an unreadable answer are
    // different facts. "no usages" invites the user to delete the method.
    let error = decode_goto(json(r#"{"locations":[]}"#)).expect_err("no legal shape");
    assert!(
        error.to_string().contains("textDocument/definition"),
        "the message must name what failed to decode, got {error}"
    );
}

#[test]
fn a_goto_response_that_is_a_number_is_an_error() {
    assert!(decode_goto(json("42")).is_err());
}

#[test]
fn a_location_array_with_one_unreadable_element_is_an_error_rather_than_a_short_list() {
    // Dropping the bad element would under-report the count silently, which is
    // exactly the failure mode this subsystem exists to avoid.
    let mixed = json(
        r#"[
            {"uri":"file:///c:/a.rs","range":{"start":{"line":0,"character":0},"end":{"line":0,"character":1}}},
            {"uri":"file:///c:/b.rs"}
        ]"#,
    );
    assert!(decode_goto(mixed).is_err());
}

// ------------------------------------------------------- decode_document_symbols

#[test]
fn the_captured_hierarchy_flattens_to_every_declaration_with_its_container_chain() {
    let symbols = decode_document_symbols(json(CAPTURED_DOCUMENT_SYMBOLS)).expect("captured");
    assert_eq!(4, symbols.len(), "namespace, class and two methods");

    assert_eq!("CodeBasics.Inspector", symbols[0].name);
    assert!(
        symbols[0].container.is_empty(),
        "the outermost node has no container"
    );

    assert_eq!("Collections", symbols[1].name);
    assert_eq!(
        vec!["CodeBasics.Inspector".to_string()],
        symbols[1].container
    );

    assert_eq!(
        vec![
            "CodeBasics.Inspector".to_string(),
            "Collections".to_string()
        ],
        symbols[2].container,
        "a method's chain is outermost first"
    );
    assert_eq!(
        vec![
            "CodeBasics.Inspector".to_string(),
            "Collections".to_string()
        ],
        symbols[3].container
    );
}

#[test]
fn a_flattened_symbol_keeps_both_the_declaration_range_and_the_identifier_range() {
    // Both are load-bearing and neither substitutes for the other: `references`
    // must be aimed at the identifier, and whether the declaration is on screen
    // is decided by the whole range.
    let symbols = decode_document_symbols(json(CAPTURED_DOCUMENT_SYMBOLS)).expect("captured");
    let method = &symbols[2];
    assert_eq!(position(25, 4), method.range.start);
    assert_eq!(position(47, 5), method.range.end);
    assert_eq!(position(25, 23), method.selection_range.start);
    assert_eq!(position(25, 37), method.selection_range.end);
}

#[test]
fn the_non_standard_glyph_field_is_ignored_rather_than_rejected() {
    // Every node the real server sent carries `glyph`, which is in no version of
    // the specification. Rejecting unknown fields would mean this decoder works
    // against the specification and not against the servers that exist.
    let with_extras = json(
        r#"[{
            "glyph": 49,
            "somethingInventedNextYear": {"a": 1},
            "name": "X",
            "kind": 5,
            "range": {"start":{"line":0,"character":0},"end":{"line":1,"character":0}},
            "selectionRange": {"start":{"line":0,"character":6},"end":{"line":0,"character":7}}
        }]"#,
    );
    let symbols = decode_document_symbols(with_extras).expect("unknown fields are ignored");
    assert_eq!(1, symbols.len());
    assert_eq!("X", symbols[0].name);
}

#[test]
fn the_signature_stays_on_the_name_because_trimming_it_is_a_presentation_choice() {
    // The picker row wants the overload; the inline row wants the bare name.
    // Deciding here would destroy information the other consumer needs.
    let symbols = decode_document_symbols(json(CAPTURED_DOCUMENT_SYMBOLS)).expect("captured");
    assert_eq!(
        "TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool",
        symbols[2].name
    );
    assert_eq!(
        Some("TryGetElements(ClrObject, IReadOnlyList<ClrObject>, int) : bool".to_string()),
        symbols[2].detail
    );
}

#[test]
fn a_hierarchical_symbol_carries_no_uri_because_it_is_the_document_we_asked_about() {
    let symbols = decode_document_symbols(json(CAPTURED_DOCUMENT_SYMBOLS)).expect("captured");
    assert!(symbols.iter().all(|s| s.uri.is_none()));
}

#[test]
fn the_flat_deprecated_symbol_information_shape_decodes_too() {
    // Still what several shipped servers answer. A decoder that only knew the
    // hierarchical shape would report "this file declares nothing".
    let flat = json(
        r#"[{
            "name": "Collections",
            "kind": 5,
            "location": {
                "uri": "file:///c:/x.cs",
                "range": {"start":{"line":18,"character":0},"end":{"line":106,"character":1}}
            }
        }]"#,
    );
    let symbols = decode_document_symbols(flat).expect("SymbolInformation[] is legal");
    assert_eq!(1, symbols.len());
    assert_eq!("Collections", symbols[0].name);
    assert_eq!(Some("file:///c:/x.cs".to_string()), symbols[0].uri);
    assert_eq!(position(18, 0), symbols[0].range.start);
}

#[test]
fn a_flat_symbol_has_no_identifier_range_so_the_declaration_range_serves_as_both() {
    // `SymbolInformation` has no `selectionRange`; there is nothing to invent
    // one from. Reusing the declaration range aims `references` at the start of
    // the declaration, which is a worse anchor and an honest one.
    let flat = json(
        r#"[{
            "name": "f",
            "kind": 12,
            "location": {
                "uri": "file:///c:/x.rs",
                "range": {"start":{"line":4,"character":0},"end":{"line":6,"character":1}}
            }
        }]"#,
    );
    let symbols = decode_document_symbols(flat).expect("legal");
    assert_eq!(symbols[0].range, symbols[0].selection_range);
}

#[test]
fn a_flat_symbols_container_name_becomes_its_container_chain() {
    let flat = json(
        r#"[{
            "name": "TryGetElements",
            "kind": 6,
            "containerName": "CodeBasics.Inspector.Collections",
            "location": {
                "uri": "file:///c:/x.cs",
                "range": {"start":{"line":25,"character":4},"end":{"line":47,"character":5}}
            }
        }]"#,
    );
    let symbols = decode_document_symbols(flat).expect("legal");
    // One element, not split on dots: the server sent one opaque string and
    // splitting it would invent a nesting the server did not state.
    assert_eq!(
        vec!["CodeBasics.Inspector.Collections".to_string()],
        symbols[0].container
    );
}

#[test]
fn a_flat_symbol_without_a_container_name_has_an_empty_chain() {
    let flat = json(
        r#"[{
            "name": "f",
            "kind": 12,
            "location": {
                "uri": "file:///c:/x.rs",
                "range": {"start":{"line":0,"character":0},"end":{"line":0,"character":1}}
            }
        }]"#,
    );
    assert!(decode_document_symbols(flat).expect("legal")[0]
        .container
        .is_empty());
}

#[test]
fn an_empty_document_symbol_array_means_this_file_declares_nothing() {
    assert!(decode_document_symbols(json("[]"))
        .expect("legal")
        .is_empty());
}

#[test]
fn a_null_document_symbol_response_means_the_same_as_an_empty_one() {
    assert!(decode_document_symbols(Value::Null)
        .expect("legal")
        .is_empty());
}

#[test]
fn a_document_symbol_response_of_no_legal_shape_is_an_error() {
    let error = decode_document_symbols(json(r#"{"symbols":[]}"#)).expect_err("no legal shape");
    assert!(
        error.to_string().contains("textDocument/documentSymbol"),
        "the message must name what failed, got {error}"
    );
}

// ------------------------------------------------------------------ symbol kinds

#[test]
fn every_lsp_symbol_kind_number_maps_to_something_without_panicking() {
    // LSP numbers kinds 1..=26 and adds to the end. Walking the whole range is
    // what proves the mapping is total rather than merely covering the kinds the
    // fixture happened to contain.
    for number in 1u32..=26 {
        let _ = symbol_kind(number);
    }
}

#[test]
fn a_symbol_kind_number_outside_the_known_range_becomes_other_rather_than_a_guess() {
    // A future kind must render as no badge, which is the same abstention
    // `symbols::declarations` already makes for a line it cannot place.
    for number in [0u32, 27, 99, u32::MAX] {
        assert_eq!(SymbolKind::Other, symbol_kind(number), "kind {number}");
    }
}

#[test]
fn the_lsp_kinds_this_app_has_a_badge_for_map_onto_the_palettes_own_kinds() {
    // Reusing `symbols::declarations::SymbolKind` is what lets the existing
    // badges and the `types.ts` mirror serve LSP results with no new wire type.
    assert_eq!(SymbolKind::Namespace, symbol_kind(3));
    assert_eq!(SymbolKind::Class, symbol_kind(5));
    assert_eq!(SymbolKind::Function, symbol_kind(6));
    assert_eq!(SymbolKind::Function, symbol_kind(9));
    assert_eq!(SymbolKind::Function, symbol_kind(12));
    assert_eq!(SymbolKind::Enum, symbol_kind(10));
    assert_eq!(SymbolKind::Interface, symbol_kind(11));
    assert_eq!(SymbolKind::Variable, symbol_kind(13));
    assert_eq!(SymbolKind::Constant, symbol_kind(14));
    assert_eq!(SymbolKind::Struct, symbol_kind(23));
}

#[test]
fn a_property_is_its_own_kind_and_a_field_is_still_not_one() {
    // 7 Property used to land on `Variable` alongside 8 Field and 13 Variable,
    // because the palette's enum had no `Property` to map to. That is a wrong
    // badge rather than no badge, which this subsystem's rule forbids: every
    // member of a C# or TypeScript class came back labelled "variable".
    assert_eq!(SymbolKind::Property, symbol_kind(7));

    // 8 Field deliberately stays `Variable`, and this is a decision rather than
    // an omission. C#, TypeScript, Java and Kotlin all distinguish a field from
    // a property in the language itself — a property has accessors and a field
    // is storage — so labelling a field "property" would be the confident wrong
    // answer this module exists to avoid. `Variable` says "named storage",
    // which a field is, and claims nothing further.
    assert_eq!(SymbolKind::Variable, symbol_kind(8));
    assert_eq!(SymbolKind::Variable, symbol_kind(13));
}

#[test]
fn the_json_document_kinds_get_no_badge_because_they_are_not_declarations() {
    // 15..=21 are String/Number/Boolean/Array/Object/Key/Null — what a server
    // answers for a `.json` file. They are values, not declarations, so a badge
    // would be a claim about source structure that is not there.
    for number in 15u32..=21 {
        assert_eq!(SymbolKind::Other, symbol_kind(number), "kind {number}");
    }
}

// ---------------------------------------------------------- server capabilities

#[test]
fn the_captured_initialize_result_decodes_to_the_capabilities_that_gate_features() {
    // This is the test that catches a real server changing shape: everything
    // asserted here was read off the wire, including the two awkward spellings
    // (`referencesProvider` as an options object, `textDocumentSync` as one).
    let capabilities =
        ServerCapabilities::from_initialize_result(&json(CAPTURED_INITIALIZE_RESULT))
            .expect("the captured result decodes");
    assert!(capabilities.references);
    assert!(capabilities.definition);
    assert!(capabilities.implementation);
    assert!(capabilities.type_definition);
    assert!(capabilities.document_symbol);
    assert_eq!(SyncKind::Incremental, capabilities.sync);
    assert_eq!(None, capabilities.position_encoding);
    assert!(capabilities.encoding_is_utf16());
}

#[test]
fn a_provider_advertised_as_an_options_object_counts_as_provided() {
    // Roslyn really does send `{"workDoneProgress":true}` here. Reading it as
    // `false` would disable "find usages" against the one server this feature
    // was built for.
    let result = json(r#"{"capabilities":{"referencesProvider":{"workDoneProgress":true}}}"#);
    assert!(
        ServerCapabilities::from_initialize_result(&result)
            .expect("legal")
            .references
    );
}

#[test]
fn a_provider_advertised_as_false_is_not_provided() {
    let result = json(r#"{"capabilities":{"implementationProvider":false}}"#);
    assert!(
        !ServerCapabilities::from_initialize_result(&result)
            .expect("legal")
            .implementation
    );
}

#[test]
fn a_provider_the_server_never_mentioned_is_not_provided() {
    // Absent means "does not provide", and the caller must be able to say "this
    // server does not do implementations" rather than showing an empty group
    // that reads as "there are none".
    let capabilities =
        ServerCapabilities::from_initialize_result(&json(r#"{"capabilities":{}}"#)).expect("legal");
    assert!(!capabilities.references);
    assert!(!capabilities.definition);
    assert!(!capabilities.implementation);
    assert!(!capabilities.type_definition);
    assert!(!capabilities.document_symbol);
}

#[test]
fn a_provider_of_a_type_the_protocol_does_not_allow_is_not_provided() {
    // Neither a bool nor an object. Claiming the feature on the strength of a
    // value we cannot read would mean sending requests the server will reject.
    let result = json(r#"{"capabilities":{"definitionProvider":"yes"}}"#);
    assert!(
        !ServerCapabilities::from_initialize_result(&result)
            .expect("legal")
            .definition
    );
}

#[test]
fn text_document_sync_as_a_bare_number_resolves_to_the_kind_it_names() {
    for (number, expected) in [
        (0, SyncKind::None),
        (1, SyncKind::Full),
        (2, SyncKind::Incremental),
    ] {
        let result = json(&format!(
            r#"{{"capabilities":{{"textDocumentSync":{number}}}}}"#
        ));
        assert_eq!(
            expected,
            ServerCapabilities::from_initialize_result(&result)
                .expect("legal")
                .sync,
            "textDocumentSync: {number}"
        );
    }
}

#[test]
fn text_document_sync_as_an_object_without_a_change_kind_is_no_sync() {
    // Per the specification `change` defaults to none. A server we cannot keep
    // in sync is refused outright upstream, so reading the default as `Full`
    // would let us send notifications it is entitled to ignore.
    let result = json(r#"{"capabilities":{"textDocumentSync":{"openClose":true}}}"#);
    assert_eq!(
        SyncKind::None,
        ServerCapabilities::from_initialize_result(&result)
            .expect("legal")
            .sync
    );
}

#[test]
fn an_absent_text_document_sync_is_no_sync_rather_than_an_error() {
    // It is a capability like any other: absent means the server does not offer
    // it, which is a refusal the caller reports, not a malformed message.
    assert_eq!(
        SyncKind::None,
        ServerCapabilities::from_initialize_result(&json(r#"{"capabilities":{}}"#))
            .expect("legal")
            .sync
    );
}

#[test]
fn a_sync_kind_number_the_protocol_does_not_define_is_no_sync() {
    let result = json(r#"{"capabilities":{"textDocumentSync":7}}"#);
    assert_eq!(
        SyncKind::None,
        ServerCapabilities::from_initialize_result(&result)
            .expect("legal")
            .sync
    );
}

#[test]
fn an_absent_position_encoding_means_utf16_and_is_not_a_refusal() {
    // The real server omits the field entirely. Treating silence as a refusal
    // would reject the only C# server there is.
    let capabilities =
        ServerCapabilities::from_initialize_result(&json(r#"{"capabilities":{}}"#)).expect("legal");
    assert_eq!(None, capabilities.position_encoding);
    assert!(capabilities.encoding_is_utf16());
}

#[test]
fn a_server_that_answers_utf16_explicitly_is_accepted() {
    let result = json(r#"{"capabilities":{"positionEncoding":"utf-16"}}"#);
    assert!(ServerCapabilities::from_initialize_result(&result)
        .expect("legal")
        .encoding_is_utf16());
}

#[test]
fn a_server_that_answers_any_other_position_encoding_is_a_refusal() {
    // We offered only `utf-16`; a server insisting on `utf-8` would have us
    // computing every column wrongly, and silently. Better to refuse it.
    let result = json(r#"{"capabilities":{"positionEncoding":"utf-8"}}"#);
    let capabilities = ServerCapabilities::from_initialize_result(&result).expect("legal");
    assert_eq!(Some("utf-8".to_string()), capabilities.position_encoding);
    assert!(!capabilities.encoding_is_utf16());
}

#[test]
fn an_initialize_result_with_no_capabilities_object_is_an_error() {
    // Not an abstention: `capabilities` is required, so its absence means we did
    // not understand the handshake at all, which is different from a server that
    // provides nothing.
    assert!(ServerCapabilities::from_initialize_result(&json(r#"{"serverInfo":{}}"#)).is_err());
}

// -------------------------------------------------------------- outgoing params

#[test]
fn initialize_params_offer_only_utf16_and_only_the_capabilities_we_implement() {
    // Pinned whole rather than field by field: an *extra* capability is the
    // dangerous change, because a server would then send us requests and
    // registrations we do not handle, and no assertion over the fields we meant
    // to declare would notice.
    //
    // # What the rename declaration invites, stated rather than assumed
    //
    // `textDocument.rename` invites **nothing new**. A rename is a *pull*: this
    // client asks and the server answers, so declaring support adds no message
    // the server may originate. `prepareSupport: true` says only that the
    // answer to a `prepareRename` we send may carry a placeholder.
    //
    // `workspace.workspaceEdit` is a **narrowing**, not a widening. Every field
    // in it removes something a rename answer would otherwise be allowed to
    // contain: no create/rename/delete operations, the legacy envelope, and no
    // claim of atomicity or undo. It invites no server→client traffic either —
    // the capability that would (`workspace/applyEdit`) is still declared
    // `false`, and `transport::answer_for` still replies `{"applied": false}`
    // to it.
    let params = initialize_params(Some(4242), "file:///c:/w", "w");
    let expected = serde_json::json!({
        "processId": 4242,
        "clientInfo": { "name": "code-basics", "version": env!("CARGO_PKG_VERSION") },
        "rootUri": "file:///c:/w",
        "workspaceFolders": [{ "uri": "file:///c:/w", "name": "w" }],
        "capabilities": {
            "general": { "positionEncodings": ["utf-16"] },
            "textDocument": {
                "synchronization": {
                    "dynamicRegistration": false,
                    "willSave": false,
                    "willSaveWaitUntil": false,
                    "didSave": false
                },
                "definition": { "linkSupport": true },
                "implementation": { "linkSupport": true },
                "typeDefinition": { "linkSupport": true },
                "references": { "dynamicRegistration": false },
                "documentSymbol": { "hierarchicalDocumentSymbolSupport": true },
                "rename": { "dynamicRegistration": false, "prepareSupport": true }
            },
            "workspace": {
                "configuration": true,
                "workspaceFolders": true,
                "applyEdit": false,
                "workspaceEdit": {
                    "documentChanges": false,
                    "resourceOperations": [],
                    "failureHandling": "abort"
                }
            },
            "window": { "workDoneProgress": true }
        }
    });
    assert_eq!(expected, params);
}

#[test]
fn initialize_params_send_a_null_process_id_when_there_is_none_to_send() {
    // The key must be present — it is how a server decides whether to exit when
    // its client dies — and `null` is the spelling for "do not watch anyone".
    let params = initialize_params(None, "file:///c:/w", "w");
    assert_eq!(Some(&Value::Null), params.get("processId"));
}

#[test]
fn did_open_params_serialise_with_the_keys_the_protocol_reads() {
    let params = DidOpenTextDocumentParams::new("file:///c:/x.cs", "csharp", 1, "class X {}");
    assert_eq!(
        serde_json::json!({
            "textDocument": {
                "uri": "file:///c:/x.cs",
                "languageId": "csharp",
                "version": 1,
                "text": "class X {}"
            }
        }),
        serde_json::to_value(&params).expect("serialises")
    );
}

#[test]
fn did_change_sends_one_event_whose_range_spans_the_whole_document() {
    // Roslyn advertises Incremental sync, so a `Full` notification is not
    // permitted; a single whole-document range is legal under Incremental and
    // keeps every notification self-describing. See `.memories/features/
    // lsp-usages/notes.md` for why that beats mapping editor deltas.
    //
    // This case passes the same text twice **because the document is not
    // changing size here**, which is the one situation where old and new extents
    // coincide. That coincidence is exactly what hid the range bug for a whole
    // feature's worth of tests — see
    // `the_replaced_range_describes_the_document_the_server_holds_not_the_new_one`.
    let params = DidChangeTextDocumentParams::whole_document(
        "file:///c:/x.cs",
        7,
        document_end("ab\ncde"),
        "ab\ncde",
    );
    assert_eq!(
        serde_json::json!({
            "textDocument": { "uri": "file:///c:/x.cs", "version": 7 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 1, "character": 3 }
                },
                "text": "ab\ncde"
            }]
        }),
        serde_json::to_value(&params).expect("serialises")
    );
}

#[test]
fn the_replaced_range_describes_the_document_the_server_holds_not_the_new_one() {
    // **The range and the text describe different documents**, and every other
    // test in this file passed the same string for both — which is exactly why
    // this shipped. A range is an instruction about the buffer the server
    // *currently* has; the text is what to put there. Measuring the range from
    // the new text is only correct when the two happen to be the same size.
    //
    // Found by running the app. Against real servers the two halves fail
    // differently, and the second is far worse:
    //
    // | edit          | range vs server's buffer | result                       |
    // |---------------|--------------------------|------------------------------|
    // | same length   | exact                    | correct, by coincidence      |
    // | one char more | overruns the end         | tsserver crashes             |
    // | shorter       | stops short              | **stale tail silently kept** |
    //
    // The shrink case answered `Ready` with a confident wrong list: a 5640-char
    // buffer replaced by a 39-char one still reported nine symbols that existed
    // only in the deleted text. That is the failure this subsystem exists to
    // refuse, so the range is now the *previous* document's extent.
    let previous_end = position(4, 11);
    let params = DidChangeTextDocumentParams::whole_document(
        "file:///c:/x.cs",
        7,
        previous_end,
        // Shorter than what the server holds, which is the dangerous direction.
        "ab",
    );
    assert_eq!(
        serde_json::json!({
            "textDocument": { "uri": "file:///c:/x.cs", "version": 7 },
            "contentChanges": [{
                "range": {
                    "start": { "line": 0, "character": 0 },
                    "end": { "line": 4, "character": 11 }
                },
                "text": "ab"
            }]
        }),
        serde_json::to_value(&params).expect("serialises"),
        "the range must span the whole of the server's document, not the new text"
    );
}

#[test]
fn the_whole_document_range_ends_on_the_empty_line_a_trailing_newline_creates() {
    // "a\n" is two lines to every editor and to LSP, the second empty. Ending on
    // line 0 would leave the newline outside the replaced range, and the server
    // would accumulate one extra line per edit.
    let params = DidChangeTextDocumentParams::whole_document(
        "file:///c:/x.cs",
        1,
        document_end("a\n"),
        "a\n",
    );
    assert_eq!(position(1, 0), params.content_changes[0].range.end);
}

#[test]
fn the_whole_document_range_of_an_empty_document_is_the_zero_position() {
    let params =
        DidChangeTextDocumentParams::whole_document("file:///c:/x.cs", 1, document_end(""), "");
    assert_eq!(position(0, 0), params.content_changes[0].range.start);
    assert_eq!(position(0, 0), params.content_changes[0].range.end);
}

#[test]
fn the_whole_document_range_counts_its_last_line_in_utf16_code_units() {
    // An emoji is one `char`, four bytes and **two** UTF-16 code units. A range
    // measured in either of the other two would fall short of the end of the
    // document and leave the server holding a tail we thought we had replaced.
    let params = DidChangeTextDocumentParams::whole_document(
        "file:///c:/x.cs",
        1,
        document_end("let e = \"🙂\""),
        "let e = \"🙂\"",
    );
    assert_eq!(position(0, 12), params.content_changes[0].range.end);
}

#[test]
fn a_carriage_return_stays_inside_the_line_it_ends() {
    // LSP splits on the line terminator, and `\r\n` is one terminator. Counting
    // the `\r` as content of the following line would put the range end one
    // column past where the server thinks the line ends.
    let params = DidChangeTextDocumentParams::whole_document(
        "file:///c:/x.cs",
        1,
        document_end("ab\r\ncd"),
        "ab\r\ncd",
    );
    assert_eq!(position(1, 2), params.content_changes[0].range.end);
}

/// The `\r\n` case above never reaches the trailing-`\r` handling at all — the
/// last segment of a split on `\n` cannot contain the `\r` that ended the line
/// before it. The only text that does reach it is a document whose *final* line
/// ends in a lone `\r`, and there the `\r` is content of the document we are
/// claiming to replace whole. Excluding it makes the replacement short by one
/// character, so the server appends our text after the `\r` it kept and gains
/// one per edit — the same accumulation the trailing-newline rule exists to
/// prevent, and invisible in exactly the same way.
#[test]
fn a_document_ending_in_a_lone_carriage_return_is_still_covered_to_its_end() {
    let params = DidChangeTextDocumentParams::whole_document(
        "file:///c:/x.cs",
        1,
        document_end("ab\r"),
        "ab\r",
    );
    assert_eq!(position(0, 3), params.content_changes[0].range.end);
}

#[test]
fn did_close_params_name_only_the_document() {
    let params = DidCloseTextDocumentParams::new("file:///c:/x.cs");
    assert_eq!(
        serde_json::json!({ "textDocument": { "uri": "file:///c:/x.cs" } }),
        serde_json::to_value(&params).expect("serialises")
    );
}

#[test]
fn references_params_ask_for_the_declaration_alongside_the_uses() {
    // The declaration is what the user middle-clicked; showing the list without
    // it makes "1 usage" and "1 usage plus the definition" look identical.
    let params = ReferenceParams::new("file:///c:/x.cs", position(25, 23), true);
    assert_eq!(
        serde_json::json!({
            "textDocument": { "uri": "file:///c:/x.cs" },
            "position": { "line": 25, "character": 23 },
            "context": { "includeDeclaration": true }
        }),
        serde_json::to_value(&params).expect("serialises")
    );
}

#[test]
fn the_three_goto_requests_share_one_params_shape() {
    // definition, implementation and typeDefinition differ only in the method
    // name, which is why there is one struct and three constants.
    let params = TextDocumentPositionParams::new("file:///c:/x.cs", position(25, 23));
    assert_eq!(
        serde_json::json!({
            "textDocument": { "uri": "file:///c:/x.cs" },
            "position": { "line": 25, "character": 23 }
        }),
        serde_json::to_value(&params).expect("serialises")
    );
}

#[test]
fn document_symbol_params_name_only_the_document() {
    let params = DocumentSymbolParams::new("file:///c:/x.cs");
    assert_eq!(
        serde_json::json!({ "textDocument": { "uri": "file:///c:/x.cs" } }),
        serde_json::to_value(&params).expect("serialises")
    );
}

#[test]
fn a_position_serialises_as_the_two_zero_based_numbers_the_protocol_expects() {
    assert_eq!(
        serde_json::json!({ "line": 0, "character": 0 }),
        serde_json::to_value(position(0, 0)).expect("serialises")
    );
}

#[test]
fn a_range_round_trips_through_the_wire_form() {
    let range = Range {
        start: position(1, 2),
        end: position(3, 4),
    };
    let wire = serde_json::to_value(range).expect("serialises");
    assert_eq!(
        serde_json::json!({
            "start": { "line": 1, "character": 2 },
            "end": { "line": 3, "character": 4 }
        }),
        wire
    );
    assert_eq!(range, serde_json::from_value(wire).expect("round trips"));
}

// ------------------------------------------------------------------- rename

/// The `textDocument/rename` answer the real Roslyn 2.140.9 gave for
/// `Walker` → `HeapWalker`, captured verbatim (probed 2026-09-04).
///
/// Two things in here are why this fixture is real and not invented, and both
/// changed the design of the feature:
///
/// * It answers **`documentChanges`** even though the client declared
///   `workspace.workspaceEdit.documentChanges: false`. There is no `changes`
///   map at all, so for this server `documentChanges` is the *only* shape that
///   arrives — the declaration buys no protection and the refusal in
///   `rename.rs` is the safeguard.
/// * Every edit is a **zero-width insertion** carrying a minimal diff:
///   `Walker` → `HeapWalker` inserts `"Heap"` rather than replacing the
///   identifier. So an insertion is the *normal* case here, not an edge case.
///
/// `"version": null` is also verbatim, which is why version cannot be used to
/// detect a stale mirror.
const CAPTURED_RENAME: &str = r#"{
  "documentChanges": [
    {
      "textDocument": { "uri": "file:///c:/w/Walker.cs", "version": null },
      "edits": [
        { "range": { "start": { "line": 30, "character": 22 },
                     "end":   { "line": 30, "character": 22 } },
          "newText": "Heap" }
      ]
    },
    {
      "textDocument": { "uri": "file:///c:/w/Program.cs", "version": null },
      "edits": [
        { "range": { "start": { "line": 160, "character": 29 },
                     "end":   { "line": 160, "character": 29 } },
          "newText": "Heap" }
      ]
    }
  ]
}"#;

fn range(start: (u32, u32), end: (u32, u32)) -> Range {
    Range {
        start: position(start.0, start.1),
        end: position(end.0, end.1),
    }
}

fn edit(start: (u32, u32), end: (u32, u32), new_text: &str) -> TextEdit {
    TextEdit {
        range: range(start, end),
        new_text: new_text.to_string(),
    }
}

#[test]
fn rename_params_put_the_new_name_on_the_wire_as_new_name_in_camel_case() {
    let params = RenameParams::new("file:///c:/x.cs", position(30, 22), "HeapWalker");
    assert_eq!(
        serde_json::json!({
            "textDocument": { "uri": "file:///c:/x.cs" },
            "position": { "line": 30, "character": 22 },
            "newName": "HeapWalker"
        }),
        serde_json::to_value(&params).expect("serialises")
    );
}

#[test]
fn prepare_rename_reuses_the_goto_position_params_rather_than_minting_a_type() {
    // One struct and one method constant, exactly as the three gotos do: the
    // request carries a document and a position and nothing else, so a
    // `PrepareRenameParams` would be a second name for the same shape.
    let params = TextDocumentPositionParams::new("file:///c:/x.cs", position(30, 22));
    assert_eq!(
        serde_json::json!({
            "textDocument": { "uri": "file:///c:/x.cs" },
            "position": { "line": 30, "character": 22 }
        }),
        serde_json::to_value(&params).expect("serialises")
    );
}

// ------------------------------------------------- decode_workspace_edit

#[test]
fn a_null_workspace_edit_is_an_empty_edit_and_not_an_error() {
    // The same rule as `decode_goto`: `null` is a real answer meaning "nothing
    // to change". An `Err` here would tell the user the rename failed when the
    // server said it had nothing to do.
    let decoded = decode_workspace_edit(Value::Null).expect("null is an answer");
    assert_eq!(WorkspaceEdit::default(), decoded);
    assert!(decoded.documents.is_empty());
    assert!(decoded.resource_operations.is_empty());
}

#[test]
fn an_empty_object_is_an_empty_edit_because_every_field_of_one_is_optional() {
    assert_eq!(
        WorkspaceEdit::default(),
        decode_workspace_edit(json("{}")).expect("an empty edit")
    );
}

#[test]
fn the_legacy_changes_map_decodes_into_one_entry_per_document() {
    let decoded = decode_workspace_edit(json(
        r#"{
          "changes": {
            "file:///c:/w/b.cs": [
              { "range": { "start": { "line": 1, "character": 0 },
                           "end":   { "line": 1, "character": 6 } },
                "newText": "HeapWalker" }
            ],
            "file:///c:/w/a.cs": [
              { "range": { "start": { "line": 2, "character": 4 },
                           "end":   { "line": 2, "character": 10 } },
                "newText": "HeapWalker" }
            ]
          }
        }"#,
    ))
    .expect("the legacy shape is legal");

    assert_eq!(
        vec![
            DocumentEdits {
                uri: "file:///c:/w/a.cs".to_string(),
                edits: vec![edit((2, 4), (2, 10), "HeapWalker")],
            },
            DocumentEdits {
                uri: "file:///c:/w/b.cs".to_string(),
                edits: vec![edit((1, 0), (1, 6), "HeapWalker")],
            },
        ],
        decoded.documents,
        "documents come out ordered by uri, whatever order the map iterated in"
    );
    assert!(decoded.resource_operations.is_empty());
}

#[test]
fn document_order_does_not_depend_on_the_order_the_server_listed_documents() {
    // A `serde_json::Map`'s iteration order is an implementation detail of a
    // dependency's feature flags, and `documentChanges` is a plain array whose
    // order is the server's. Neither may reach the result: a caller reporting
    // "6 files" wants them in one stable order, and a test over an unstable one
    // passes or fails by luck.
    let reversed = decode_workspace_edit(json(
        r#"{
          "documentChanges": [
            { "textDocument": { "uri": "file:///c:/w/z.cs", "version": null }, "edits": [] },
            { "textDocument": { "uri": "file:///c:/w/m.cs", "version": null }, "edits": [] },
            { "textDocument": { "uri": "file:///c:/w/a.cs", "version": null }, "edits": [] }
          ]
        }"#,
    ))
    .expect("legal");
    let uris: Vec<&str> = reversed
        .documents
        .iter()
        .map(|document| document.uri.as_str())
        .collect();
    assert_eq!(
        vec![
            "file:///c:/w/a.cs",
            "file:///c:/w/m.cs",
            "file:///c:/w/z.cs"
        ],
        uris
    );
}

#[test]
fn the_real_roslyn_rename_answer_decodes_to_two_documents_and_no_operations() {
    let decoded = decode_workspace_edit(json(CAPTURED_RENAME)).expect("the real answer decodes");

    assert_eq!(
        vec![
            DocumentEdits {
                uri: "file:///c:/w/Program.cs".to_string(),
                edits: vec![edit((160, 29), (160, 29), "Heap")],
            },
            DocumentEdits {
                uri: "file:///c:/w/Walker.cs".to_string(),
                edits: vec![edit((30, 22), (30, 22), "Heap")],
            },
        ],
        decoded.documents
    );
    assert!(
        decoded.resource_operations.is_empty(),
        "renaming a type declared in a same-named file provoked no file rename"
    );
    assert!(
        decoded.documents.iter().all(|document| document
            .edits
            .iter()
            .all(|edit| edit.range.start == edit.range.end)),
        "every edit the real server sent is a zero-width insertion — this is \
         the normal case for Roslyn, not an edge case"
    );
}

#[test]
fn a_null_document_version_is_not_read_as_a_stale_mirror_signal() {
    // Roslyn sends `"version": null` on every entry, so there is nothing here
    // to compare against the buffer's version. The field is dropped rather than
    // decoded into an `Option` nobody could act on — the flush-before-asking
    // ordering is what protects against a stale mirror.
    let decoded = decode_workspace_edit(json(CAPTURED_RENAME)).expect("decodes");
    assert_eq!(2, decoded.documents.len());
}

#[test]
fn several_document_changes_entries_for_one_uri_merge_into_one_document() {
    // Two entries naming one file would otherwise be planned and applied
    // separately, and the second application would be computed against text the
    // first had already changed. Merging is what lets `edits::plan` see the
    // whole set for a file at once and refuse an overlap.
    let decoded = decode_workspace_edit(json(
        r#"{
          "documentChanges": [
            { "textDocument": { "uri": "file:///c:/w/a.cs" },
              "edits": [ { "range": { "start": { "line": 1, "character": 0 },
                                      "end":   { "line": 1, "character": 1 } },
                           "newText": "x" } ] },
            { "textDocument": { "uri": "file:///c:/w/a.cs" },
              "edits": [ { "range": { "start": { "line": 5, "character": 0 },
                                      "end":   { "line": 5, "character": 1 } },
                           "newText": "y" } ] }
          ]
        }"#,
    ))
    .expect("legal");

    assert_eq!(1, decoded.documents.len());
    assert_eq!(
        vec![edit((1, 0), (1, 1), "x"), edit((5, 0), (5, 1), "y")],
        decoded.documents[0].edits
    );
}

#[test]
fn an_annotated_text_edit_keeps_its_edit_and_drops_the_annotation_id() {
    // `AnnotatedTextEdit` is a `TextEdit` plus an `annotationId` naming an entry
    // in `changeAnnotations`. This app asks no confirmation questions, so the
    // annotation is nothing it can act on — but the edit itself is ordinary and
    // rejecting the shape would refuse a legal answer.
    let decoded = decode_workspace_edit(json(
        r#"{
          "changeAnnotations": { "rename": { "label": "Rename", "needsConfirmation": true } },
          "documentChanges": [
            { "textDocument": { "uri": "file:///c:/w/a.cs", "version": 3 },
              "edits": [ { "range": { "start": { "line": 0, "character": 0 },
                                      "end":   { "line": 0, "character": 3 } },
                           "newText": "Bar",
                           "annotationId": "rename" } ] }
          ]
        }"#,
    ))
    .expect("an annotated edit is a legal edit");

    assert_eq!(
        vec![DocumentEdits {
            uri: "file:///c:/w/a.cs".to_string(),
            edits: vec![edit((0, 0), (0, 3), "Bar")],
        }],
        decoded.documents
    );
}

#[test]
fn both_shapes_present_prefers_document_changes() {
    // The specification says a client that declared `documentChanges` support
    // must ignore `changes`, and a server sending both has already decided which
    // is authoritative. Reading the legacy map would apply the poorer of two
    // answers — and the two need not agree.
    let decoded = decode_workspace_edit(json(
        r#"{
          "changes": {
            "file:///c:/w/legacy.cs": [
              { "range": { "start": { "line": 0, "character": 0 },
                           "end":   { "line": 0, "character": 1 } },
                "newText": "legacy" }
            ]
          },
          "documentChanges": [
            { "textDocument": { "uri": "file:///c:/w/modern.cs" },
              "edits": [ { "range": { "start": { "line": 0, "character": 0 },
                                      "end":   { "line": 0, "character": 1 } },
                           "newText": "modern" } ] }
          ]
        }"#,
    ))
    .expect("legal");

    assert_eq!(1, decoded.documents.len());
    assert_eq!("file:///c:/w/modern.cs", decoded.documents[0].uri);
}

#[test]
fn each_resource_operation_kind_lands_in_resource_operations_and_is_never_dropped() {
    // These are the operations this app declines to perform — renaming a file
    // moves something the editor may have open and invalidates tab ids, the
    // symbol index and every `EditorSource`. Dropping them here would leave the
    // caller looking at a text-only edit set that *is* applicable, and it would
    // apply half a rename. So they are decoded, not filtered, and `rename.rs`
    // refuses the whole answer when the list is non-empty.
    let decoded = decode_workspace_edit(json(
        r#"{
          "documentChanges": [
            { "kind": "create", "uri": "file:///c:/w/new.cs" },
            { "textDocument": { "uri": "file:///c:/w/a.cs" },
              "edits": [ { "range": { "start": { "line": 0, "character": 0 },
                                      "end":   { "line": 0, "character": 1 } },
                           "newText": "x" } ] },
            { "kind": "rename", "oldUri": "file:///c:/w/old.cs", "newUri": "file:///c:/w/renamed.cs" },
            { "kind": "delete", "uri": "file:///c:/w/gone.cs" }
          ]
        }"#,
    ))
    .expect("a legible answer, even though it will be declined");

    assert_eq!(
        vec![
            ResourceOperation {
                kind: "create".to_string(),
                uris: vec!["file:///c:/w/new.cs".to_string()],
            },
            ResourceOperation {
                kind: "rename".to_string(),
                uris: vec![
                    "file:///c:/w/old.cs".to_string(),
                    "file:///c:/w/renamed.cs".to_string(),
                ],
            },
            ResourceOperation {
                kind: "delete".to_string(),
                uris: vec!["file:///c:/w/gone.cs".to_string()],
            },
        ],
        decoded.resource_operations,
        "a rename operation keeps both uris, old first, so the refusal can name \
         what the server wanted to move"
    );
    assert_eq!(
        1,
        decoded.documents.len(),
        "the text edits alongside them are still decoded — the caller refuses \
         the whole answer, which it cannot do if half of it is missing"
    );
}

#[test]
fn an_operation_kind_this_module_does_not_know_is_still_reported_as_an_operation() {
    // A kind added to the protocol after this was written is not an unreadable
    // shape and it is certainly not a text edit. Dropping it would silently
    // convert an answer this app must decline into one it would apply.
    let decoded = decode_workspace_edit(json(
        r#"{ "documentChanges": [ { "kind": "copy", "uri": "file:///c:/w/a.cs" } ] }"#,
    ))
    .expect("legible");
    assert_eq!(
        vec![ResourceOperation {
            kind: "copy".to_string(),
            uris: vec!["file:///c:/w/a.cs".to_string()],
        }],
        decoded.resource_operations
    );
}

#[test]
fn a_resource_operation_carries_no_new_decode_error_variant() {
    // Deliberate: a resource operation is not an unreadable shape, it is a
    // legible request this app declines, and the decline belongs where the
    // user-facing sentence is written. `DecodeError` therefore still has exactly
    // one variant.
    let error = decode_workspace_edit(json(r#"{"documentChanges": 7}"#))
        .expect_err("a number is not a document change list");
    assert!(matches!(error, DecodeError::Shape { .. }));
}

#[test]
fn a_workspace_edit_shape_the_protocol_does_not_allow_is_an_error_not_an_empty_edit() {
    // Every one of these would otherwise read as "the server had nothing to
    // rename", which invites the user to conclude the symbol is unused.
    for text in [
        r#"[]"#,
        r#""nope""#,
        r#"7"#,
        r#"true"#,
        r#"{"changes": []}"#,
        r#"{"changes": {"file:///a": 7}}"#,
        r#"{"changes": {"file:///a": [{"newText": "x"}]}}"#,
        r#"{"documentChanges": [{}]}"#,
        r#"{"documentChanges": [{"textDocument": {"uri": "file:///a"}}]}"#,
        r#"{"documentChanges": [{"textDocument": {}, "edits": []}]}"#,
        r#"{"documentChanges": [{"textDocument": {"uri": "file:///a"}, "edits": 7}]}"#,
        r#"{"documentChanges": [{"textDocument": {"uri": "file:///a"}, "edits": [{"range": {}, "newText": "x"}]}]}"#,
    ] {
        let outcome = decode_workspace_edit(json(text));
        assert!(
            matches!(outcome, Err(DecodeError::Shape { .. })),
            "{text} decoded to {outcome:?} rather than being refused"
        );
    }
}

#[test]
fn the_workspace_edit_refusal_names_the_request_that_could_not_be_read() {
    let error = decode_workspace_edit(json(r#"{"documentChanges": 7}"#)).expect_err("unreadable");
    assert!(
        error.to_string().contains("textDocument/rename"),
        "the sentence must say which question went unanswered: {error}"
    );
}

#[test]
fn a_null_valued_shape_key_is_read_as_absent_rather_than_as_an_unreadable_shape() {
    // `"changes": null` and `"documentChanges": null` are what a server that
    // builds its answer from optional fields emits. Both mean "no edits of this
    // kind", and refusing them would report a failed rename for an answer that
    // said nothing was needed.
    for text in [
        r#"{"documentChanges": null}"#,
        r#"{"changes": null}"#,
        r#"{"changes": null, "documentChanges": null}"#,
    ] {
        assert_eq!(
            WorkspaceEdit::default(),
            decode_workspace_edit(json(text)).unwrap_or_else(|error| panic!("{text}: {error}")),
        );
    }
}

#[test]
fn document_changes_being_null_falls_back_to_the_legacy_changes_map() {
    let decoded = decode_workspace_edit(json(
        r#"{
          "documentChanges": null,
          "changes": {
            "file:///c:/w/a.cs": [
              { "range": { "start": { "line": 0, "character": 0 },
                           "end":   { "line": 0, "character": 1 } },
                "newText": "x" }
            ]
          }
        }"#,
    ))
    .expect("legal");
    assert_eq!(1, decoded.documents.len());
}

#[test]
fn an_empty_edit_list_for_a_document_is_kept_rather_than_pruned() {
    // The server named a file and said it needs no changes. Pruning it would
    // make a caller counting documents disagree with a caller counting edits,
    // and the two disagreeing silently is how a rename reports the wrong scope.
    let decoded =
        decode_workspace_edit(json(r#"{"changes": {"file:///c:/w/a.cs": []}}"#)).expect("legal");
    assert_eq!(
        vec![DocumentEdits {
            uri: "file:///c:/w/a.cs".to_string(),
            edits: Vec::new(),
        }],
        decoded.documents
    );
}

// -------------------------------------------------- decode_prepare_rename

#[test]
fn a_null_prepare_rename_answer_means_the_caret_is_not_on_something_renameable() {
    // A real answer, not a failure and not an empty range: the caret is on a
    // keyword, a comment or a literal. The three read identically if this
    // collapses into an error, and only this one should keep the field closed
    // with a sentence rather than opening it empty.
    assert_eq!(
        PrepareRenameResponse::NotRenameable,
        decode_prepare_rename(Value::Null).expect("null is an answer")
    );
}

#[test]
fn the_real_roslyn_prepare_rename_answer_is_a_bare_range_with_no_placeholder() {
    // Captured 2026-09-04 for a C# type: `{start, end}` at the top level, no
    // `placeholder`. So deriving the field's prefill from the buffer is the
    // *live* path for C#, not a fallback — a decoder handling only the
    // `{range, placeholder}` shape would open an empty rename box.
    let decoded = decode_prepare_rename(json(
        r#"{ "start": {"line":30,"character":22}, "end": {"line":30,"character":28} }"#,
    ))
    .expect("the real answer decodes");
    assert_eq!(
        PrepareRenameResponse::Range {
            range: range((30, 22), (30, 28)),
            placeholder: None,
        },
        decoded
    );
}

#[test]
fn a_range_with_a_placeholder_keeps_the_placeholder_the_server_chose() {
    let decoded = decode_prepare_rename(json(
        r#"{ "range": { "start": {"line":1,"character":2}, "end": {"line":1,"character":5} },
             "placeholder": "Walker" }"#,
    ))
    .expect("legal");
    assert_eq!(
        PrepareRenameResponse::Range {
            range: range((1, 2), (1, 5)),
            placeholder: Some("Walker".to_string()),
        },
        decoded
    );
}

#[test]
fn a_wrapped_range_with_no_placeholder_is_still_a_range() {
    let decoded = decode_prepare_rename(json(
        r#"{ "range": { "start": {"line":1,"character":2}, "end": {"line":1,"character":5} } }"#,
    ))
    .expect("legal");
    assert_eq!(
        PrepareRenameResponse::Range {
            range: range((1, 2), (1, 5)),
            placeholder: None,
        },
        decoded
    );
}

#[test]
fn default_behavior_is_the_same_answer_whether_the_server_says_true_or_false() {
    // Both are legal, and `{"defaultBehavior": false}` does *not* mean "not
    // renameable" — it is still the "no special range" answer. Reading it as a
    // refusal would close the field on a symbol the server would happily rename.
    for text in [
        r#"{"defaultBehavior": true}"#,
        r#"{"defaultBehavior": false}"#,
    ] {
        assert_eq!(
            PrepareRenameResponse::DefaultBehavior,
            decode_prepare_rename(json(text)).unwrap_or_else(|error| panic!("{text}: {error}")),
            "for {text}"
        );
    }
}

#[test]
fn a_prepare_rename_shape_the_protocol_does_not_allow_is_an_error() {
    for text in [
        r#"7"#,
        r#""Walker""#,
        r#"[]"#,
        r#"{}"#,
        r#"{"placeholder": "Walker"}"#,
        r#"{"start": {"line":1,"character":2}}"#,
        r#"{"range": 7}"#,
        r#"{"defaultBehavior": "yes"}"#,
    ] {
        let outcome = decode_prepare_rename(json(text));
        assert!(
            matches!(outcome, Err(DecodeError::Shape { .. })),
            "{text} decoded to {outcome:?} rather than being refused"
        );
    }
}

#[test]
fn the_prepare_rename_refusal_names_the_request_that_could_not_be_read() {
    let error = decode_prepare_rename(json(r#"7"#)).expect_err("unreadable");
    assert!(
        error.to_string().contains("textDocument/prepareRename"),
        "the sentence must say which question went unanswered: {error}"
    );
}

// ------------------------------------------------------- rename capability

#[test]
fn the_real_roslyn_capability_set_advertises_rename_and_prepare_rename() {
    // Measured against the real server 2026-09-04, which is why this fixture is
    // not a fiction: `renameProvider` really is `{"prepareProvider": true}`, so
    // F2 works for C# and both requests are available.
    let capabilities =
        ServerCapabilities::from_initialize_result(&json(CAPTURED_INITIALIZE_RESULT))
            .expect("the real handshake decodes");
    assert!(capabilities.rename);
    assert!(capabilities.prepare_rename);
}

#[test]
fn rename_provider_true_gives_rename_without_prepare_rename() {
    // The reason these are two fields and not one. A server advertising
    // `renameProvider: true` with no `prepareProvider` answers `-32601` to
    // `prepareRename`, which arrives as `RequestError::Failed` — and gating the
    // rename on that failure would refuse a rename that would have worked.
    let capabilities = ServerCapabilities::from_initialize_result(&json(
        r#"{"capabilities": {"textDocumentSync": 2, "renameProvider": true}}"#,
    ))
    .expect("decodes");
    assert!(capabilities.rename);
    assert!(!capabilities.prepare_rename);
}

#[test]
fn a_rename_provider_options_object_without_prepare_provider_provides_rename_only() {
    // The shape Roslyn uses for its other providers: `{"workDoneProgress": true}`.
    // `provides()` already reads an options object as yes, and the absence of
    // `prepareProvider` inside it is a real "no".
    let capabilities = ServerCapabilities::from_initialize_result(&json(
        r#"{"capabilities": {"textDocumentSync": 2,
             "renameProvider": {"workDoneProgress": true}}}"#,
    ))
    .expect("decodes");
    assert!(capabilities.rename);
    assert!(!capabilities.prepare_rename);
}

#[test]
fn no_rename_provider_at_all_provides_neither() {
    let capabilities = ServerCapabilities::from_initialize_result(&json(
        r#"{"capabilities": {"textDocumentSync": 2}}"#,
    ))
    .expect("decodes");
    assert!(!capabilities.rename);
    assert!(!capabilities.prepare_rename);
}

#[test]
fn prepare_provider_is_true_only_for_a_literal_true_inside_an_options_object() {
    // Everything else leaves us without a claim, and claiming it means sending a
    // request the server will reject. `renameProvider: true` in particular is
    // *not* a promise about `prepareRename`.
    for text in [
        r#"{"capabilities": {"textDocumentSync": 2, "renameProvider": {"prepareProvider": false}}}"#,
        r#"{"capabilities": {"textDocumentSync": 2, "renameProvider": {"prepareProvider": "yes"}}}"#,
        r#"{"capabilities": {"textDocumentSync": 2, "renameProvider": {"prepareProvider": 1}}}"#,
        r#"{"capabilities": {"textDocumentSync": 2, "renameProvider": false}}"#,
        r#"{"capabilities": {"textDocumentSync": 2, "renameProvider": "yes"}}"#,
    ] {
        let capabilities =
            ServerCapabilities::from_initialize_result(&json(text)).expect("decodes");
        assert!(
            !capabilities.prepare_rename,
            "{text} must not read as a prepareRename promise"
        );
    }
}

#[test]
fn the_client_never_declares_that_it_will_do_file_operations() {
    // A standalone assertion beside the whole-JSON pin on purpose: the next
    // legitimate widening of that blob would carry this guarantee away with it
    // silently, because the pin's failure message says "the JSON changed" and
    // not "you just promised to move the user's files".
    //
    // `resourceOperations: []` is *stated* rather than omitted because omission
    // and "none" read identically to a human and only one of them is a promise —
    // the same reason `applyEdit: false` is declared.
    let params = initialize_params(Some(1), "file:///c:/w", "w");
    let workspace_edit = params
        .pointer("/capabilities/workspace/workspaceEdit")
        .expect("the client declares its workspaceEdit support");

    assert_eq!(
        Some(&serde_json::json!([])),
        workspace_edit.get("resourceOperations"),
        "an empty list is the declaration that this client creates, renames and \
         deletes no files"
    );
    assert_eq!(
        Some(&Value::String("abort".to_string())),
        workspace_edit.get("failureHandling"),
        "`abort` is the only true value: `transactional` claims cross-file \
         atomicity nothing here can promise once write 7 has landed, and `undo` \
         claims an undo this app does not have"
    );
    assert_eq!(
        Some(&Value::Bool(false)),
        params.pointer("/capabilities/workspace/applyEdit"),
        "`applyEdit: false` is about a server *pushing* an edit and stays false; \
         rename is a pull, so the two are not in tension"
    );
}
