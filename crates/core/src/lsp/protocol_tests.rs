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
                "documentSymbol": { "hierarchicalDocumentSymbolSupport": true }
            },
            "workspace": {
                "configuration": true,
                "workspaceFolders": true,
                "applyEdit": false
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
    let params = DidChangeTextDocumentParams::whole_document("file:///c:/x.cs", 7, "ab\ncde");
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
fn the_whole_document_range_ends_on_the_empty_line_a_trailing_newline_creates() {
    // "a\n" is two lines to every editor and to LSP, the second empty. Ending on
    // line 0 would leave the newline outside the replaced range, and the server
    // would accumulate one extra line per edit.
    let params = DidChangeTextDocumentParams::whole_document("file:///c:/x.cs", 1, "a\n");
    assert_eq!(position(1, 0), params.content_changes[0].range.end);
}

#[test]
fn the_whole_document_range_of_an_empty_document_is_the_zero_position() {
    let params = DidChangeTextDocumentParams::whole_document("file:///c:/x.cs", 1, "");
    assert_eq!(position(0, 0), params.content_changes[0].range.start);
    assert_eq!(position(0, 0), params.content_changes[0].range.end);
}

#[test]
fn the_whole_document_range_counts_its_last_line_in_utf16_code_units() {
    // An emoji is one `char`, four bytes and **two** UTF-16 code units. A range
    // measured in either of the other two would fall short of the end of the
    // document and leave the server holding a tail we thought we had replaced.
    let params =
        DidChangeTextDocumentParams::whole_document("file:///c:/x.cs", 1, "let e = \"🙂\"");
    assert_eq!(position(0, 12), params.content_changes[0].range.end);
}

#[test]
fn a_carriage_return_stays_inside_the_line_it_ends() {
    // LSP splits on the line terminator, and `\r\n` is one terminator. Counting
    // the `\r` as content of the following line would put the range end one
    // column past where the server thinks the line ends.
    let params = DidChangeTextDocumentParams::whole_document("file:///c:/x.cs", 1, "ab\r\ncd");
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
    let params = DidChangeTextDocumentParams::whole_document("file:///c:/x.cs", 1, "ab\r");
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
