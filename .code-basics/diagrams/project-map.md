---
code-basics: v1
derivation: user
---

%% Derived by code-basics from the files on disk (scanner version 2).
%% cb-app: src-tauri/tauri.conf.json names relations between parts of this workspace that this graph has no edge kind for, so they were not drawn — it bundles the frontend built at '../dist' (build.frontendDist); it ships 'resources/inspector/' as bundled resources (bundle.resources); it runs another part of this workspace to produce that frontend (build.beforeBuildCommand, whose text is not quoted here because a command line can carry a credential). A frontend reached by run-time IPC and a file copied into a bundle are not compile-time references, and drawing either as an arrow would claim more than the file says
flowchart LR
    nroot["code-basics (package.json)"]
    nsidecar_2d_fixtures_2d_Crasher_2d_Crasher_2e_csproj["Crasher"]
    nsidecar_2d_inspector_2d_Inspector_2e_csproj["Inspector"]
    subgraph nworkspace_3a_Cargo_2e_toml["code-basics (Cargo.toml)"]
        ncrates_2d_core["cb-core"]
        nsrc_2d_tauri["cb-app"]
    end
    nsrc_2d_tauri --> ncrates_2d_core
    subgraph legend["Legend"]
        legend_project["project in this workspace"]
        legend_from["A"] -->|"project reference"| legend_to["B"]
    end
