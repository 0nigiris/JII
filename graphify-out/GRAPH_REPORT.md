# Graph Report - .  (2026-07-16)

## Corpus Check
- 66 files · ~189,580 words
- Verdict: corpus is large enough that graph structure adds value.

## Summary
- 1501 nodes · 4471 edges · 78 communities (61 shown, 17 thin omitted)
- Extraction: 97% EXTRACTED · 3% INFERRED · 0% AMBIGUOUS · INFERRED: 143 edges (avg confidence: 0.8)
- Token cost: 412,698 input · 0 output

## Community Hubs (Navigation)
- [[_COMMUNITY_CLI Command Dispatch|CLI Command Dispatch]]
- [[_COMMUNITY_Privileged Exec & Archives|Privileged Exec & Archives]]
- [[_COMMUNITY_Architecture Decision Records|Architecture Decision Records]]
- [[_COMMUNITY_Config & Runtime Types|Config & Runtime Types]]
- [[_COMMUNITY_CLI Integration Tests|CLI Integration Tests]]
- [[_COMMUNITY_Project Constraints & Handoff Docs|Project Constraints & Handoff Docs]]
- [[_COMMUNITY_Supported Systems & Release Testing|Supported Systems & Release Testing]]
- [[_COMMUNITY_DNF Provider|DNF Provider]]
- [[_COMMUNITY_Gentoo Provider|Gentoo Provider]]
- [[_COMMUNITY_Self-update & Asset Verification|Self-update & Asset Verification]]
- [[_COMMUNITY_COPR Provider (search)|COPR Provider (search)]]
- [[_COMMUNITY_COPR Provider (planning)|COPR Provider (planning)]]
- [[_COMMUNITY_Forge Asset Classification|Forge Asset Classification]]
- [[_COMMUNITY_APT Provider|APT Provider]]
- [[_COMMUNITY_Homebrew Provider|Homebrew Provider]]
- [[_COMMUNITY_pipx Provider|pipx Provider]]
- [[_COMMUNITY_Void Provider|Void Provider]]
- [[_COMMUNITY_Cargo Provider|Cargo Provider]]
- [[_COMMUNITY_Forge Install Planning|Forge Install Planning]]
- [[_COMMUNITY_Nix Config Editing|Nix Config Editing]]
- [[_COMMUNITY_Snap Provider|Snap Provider]]
- [[_COMMUNITY_GitHub Forge & Repo Search|GitHub Forge & Repo Search]]
- [[_COMMUNITY_Go Provider|Go Provider]]
- [[_COMMUNITY_Pacman Provider|Pacman Provider]]
- [[_COMMUNITY_Zypper Provider|Zypper Provider]]
- [[_COMMUNITY_AUR Provider|AUR Provider]]
- [[_COMMUNITY_Engine Core (searchrankbootstrap)|Engine Core (search/rank/bootstrap)]]
- [[_COMMUNITY_Registry (JSON state)|Registry (JSON state)]]
- [[_COMMUNITY_Provider Trait & Registry|Provider Trait & Registry]]
- [[_COMMUNITY_Ranking Logic|Ranking Logic]]
- [[_COMMUNITY_Package Model & Spec Grammar|Package Model & Spec Grammar]]
- [[_COMMUNITY_Recommend Catalog|Recommend Catalog]]
- [[_COMMUNITY_Search Cache|Search Cache]]
- [[_COMMUNITY_Flatpak Provider|Flatpak Provider]]
- [[_COMMUNITY_Platform Detection|Platform Detection]]
- [[_COMMUNITY_Provider Catalog (concepts)|Provider Catalog (concepts)]]
- [[_COMMUNITY_Audit & Verification|Audit & Verification]]
- [[_COMMUNITY_npm Provider|npm Provider]]
- [[_COMMUNITY_Charts Generation Script|Charts Generation Script]]
- [[_COMMUNITY_Provider Info & Ecosystem|Provider Info & Ecosystem]]
- [[_COMMUNITY_Nix Provider Commands|Nix Provider Commands]]
- [[_COMMUNITY_npm Manifest Parsing|npm Manifest Parsing]]
- [[_COMMUNITY_Plan JSON Serialization|Plan JSON Serialization]]
- [[_COMMUNITY_UI Palette|UI Palette]]
- [[_COMMUNITY_Declarative Install Strategies|Declarative Install Strategies]]
- [[_COMMUNITY_Installed Resolution & Batch|Installed Resolution & Batch]]
- [[_COMMUNITY_Flatpak Plans|Flatpak Plans]]
- [[_COMMUNITY_HTTP Client & Probe|HTTP Client & Probe]]
- [[_COMMUNITY_Source Catalog & Trust|Source Catalog & Trust]]
- [[_COMMUNITY_install.sh Script|install.sh Script]]
- [[_COMMUNITY_Nix Batch & Diff|Nix Batch & Diff]]
- [[_COMMUNITY_Search & GitHub Features|Search & GitHub Features]]
- [[_COMMUNITY_Source Availability Checks|Source Availability Checks]]
- [[_COMMUNITY_Bootstrap & Setup Features|Bootstrap & Setup Features]]
- [[_COMMUNITY_Source Health  doctor|Source Health / doctor]]
- [[_COMMUNITY_Record Batching|Record Batching]]
- [[_COMMUNITY_Plan & Execution Concepts|Plan & Execution Concepts]]
- [[_COMMUNITY_PkgVersion Type|PkgVersion Type]]
- [[_COMMUNITY_Candidate Selection Model|Candidate Selection Model]]
- [[_COMMUNITY_Brand Assets|Brand Assets]]
- [[_COMMUNITY_Project State Docs|Project State Docs]]
- [[_COMMUNITY_UI Progress & Output|UI Progress & Output]]
- [[_COMMUNITY_Homebrew Formula|Homebrew Formula]]
- [[_COMMUNITY_Charts Workflow|Charts Workflow]]
- [[_COMMUNITY_Brand Charts|Brand Charts]]
- [[_COMMUNITY_Install & Packaging|Install & Packaging]]
- [[_COMMUNITY_i18n & Output Modes|i18n & Output Modes]]
- [[_COMMUNITY_Platform Seam|Platform Seam]]
- [[_COMMUNITY_Update Commands|Update Commands]]
- [[_COMMUNITY_Repo as Single Source of Truth|Repo as Single Source of Truth]]
- [[_COMMUNITY_ADR-0001 Single Crate|ADR-0001 Single Crate]]
- [[_COMMUNITY_ADR-0002 JSON State|ADR-0002 JSON State]]
- [[_COMMUNITY_ADR-0008 COPR Phase 4|ADR-0008 COPR Phase 4]]
- [[_COMMUNITY_ADR-0013 Minimal CI|ADR-0013 Minimal CI]]
- [[_COMMUNITY_ADR-0042 Search Matching|ADR-0042 Search Matching]]
- [[_COMMUNITY_ADR-0051 First-run Setup|ADR-0051 First-run Setup]]
- [[_COMMUNITY_ADR-0060 JII Everywhere|ADR-0060 JII Everywhere]]
- [[_COMMUNITY_Version Chooser (deferred)|Version Chooser (deferred)]]

## God Nodes (most connected - your core abstractions)
1. `InstallPlan` - 158 edges
2. `PackageCandidate` - 128 edges
3. `InstalledRecord` - 125 edges
4. `Renderer` - 84 edges
5. `Cli` - 69 edges
6. `Engine` - 66 edges
7. `Config` - 63 edges
8. `TrustLevel` - 40 edges
9. `command_plan()` - 40 edges
10. `Query` - 28 edges

## Surprising Connections (you probably didn't know these)
- `JII self-update` --semantically_similar_to--> `Distribution of JII itself (COPR)`  [INFERRED] [semantically similar]
  README.md → docs/ARCHITECTURE.md
- `Philosophy — user thinks about software` --semantically_similar_to--> `Design Principles (binding)`  [INFERRED] [semantically similar]
  README.md → docs/ARCHITECTURE.md
- `Supported Systems & Smoke Test` --semantically_similar_to--> `Packaging JII (channels guide)`  [INFERRED] [semantically similar]
  docs/SUPPORTED_SYSTEMS.md → packaging/README.md
- `JII is never fully run as root` --conceptually_related_to--> `Binding MVP Constraints`  [INFERRED]
  docs/ARCHITECTURE.md → CLAUDE.md
- `JSON State File (not SQLite yet)` --conceptually_related_to--> `JSON Registry + Verification`  [INFERRED]
  CLAUDE.md → docs/ARCHITECTURE.md

## Import Cycles
- 1-file cycle: `src/model.rs -> src/model.rs`

## Hyperedges (group relationships)
- **JII Install Pipeline (search→rank→plan→execute)** — docs_architecture_pipeline, docs_architecture_provider_trait, docs_architecture_ranking, docs_architecture_install_plan, docs_architecture_exec, docs_architecture_engine [EXTRACTED 0.90]
- **Trust & Safety Model** — docs_architecture_trust_security, claude_trust_levels, docs_architecture_default_yes_threshold, docs_architecture_artifact_verification, docs_architecture_never_root [EXTRACTED 0.85]
- **Agent Onboarding Doc Set** — agents_onboarding, claude_md, docs_architecture, agents_ai_handoff_policy [EXTRACTED 0.80]
- **Load-bearing architecture invariants** — docs_decisions_adr_0003, docs_decisions_adr_0004, docs_decisions_adr_0005, docs_decisions_adr_0006, docs_decisions_adr_0020 [EXTRACTED 0.90]
- **Grow via optional Provider capabilities pattern** — docs_decisions_adr_0022, docs_decisions_adr_0025, docs_decisions_adr_0034, docs_decisions_adr_0036, docs_decisions_adr_0037, docs_decisions_adr_0045, docs_decisions_adr_0054 [EXTRACTED 0.85]
- **Terminal 1.0 track delivery plan (T1-T6)** — docs_roadmap_terminal_1_0, docs_decisions_adr_0026, docs_decisions_adr_0025, docs_decisions_adr_0029, docs_decisions_adr_0053, docs_decisions_adr_0065 [INFERRED 0.80]
- **Search-Rank-Plan-Execute Pipeline** — docs_tasks_engine, docs_tasks_provider_trait, docs_tasks_ranking, docs_tasks_install_plan, docs_tasks_action_enum [EXTRACTED 0.85]
- **Package Spec Unifies Source Selection Across Verbs** — docs_ux_evaluation_package_spec, docs_tasks_candidate_chooser, docs_tasks_version_chooser [EXTRACTED 0.80]
- **Bootstrap-Missing-Manager Flow** — docs_ai_context_bootstrap_managers, docs_ai_context_can_search, docs_ai_context_jii_sources [EXTRACTED 0.80]
- **All JII packaging channels** — packaging_readme_copr_channel, packaging_readme_obs_channel, packaging_readme_aur_channel, packaging_readme_alpine_channel, packaging_readme_void_channel, packaging_readme_gentoo_channel, packaging_readme_nix_channel, packaging_readme_homebrew_channel, packaging_readme_crates_channel [EXTRACTED 1.00]
- **Release pipeline (build to publish)** — workflows_release_build_job, workflows_release_nfpm_step, workflows_release_publish_job [EXTRACTED 1.00]
- **Native package-manager providers** — docs_supported_systems_dnf_provider, docs_supported_systems_apt_provider, docs_supported_systems_pacman_provider, docs_supported_systems_zypper_provider, docs_supported_systems_void_provider, docs_supported_systems_gentoo_provider, docs_supported_systems_nix_provider [EXTRACTED 1.00]

## Communities (78 total, 17 thin omitted)

### Community 0 - "CLI Command Dispatch"
Cohesion: 0.07
Nodes (21): Cli, record_batch_names(), refresh_repo_metadata(), run_plain_command(), run_shell_command(), BatchPlan, Engine, RecordBatchPlan (+13 more)

### Community 1 - "Privileged Exec & Archives"
Cohesion: 0.06
Nodes (42): jii_backup_path(), root_tmp_path(), root_write_argv(), root_write_argv_shows_backup_then_write_with_elevation(), write_nix_config_root(), Path, ArchiveFile, basename() (+34 more)

### Community 2 - "Architecture Decision Records"
Cohesion: 0.06
Nodes (63): JII Road to first public Beta, First public Beta, JII Architecture Decision Records, ADR-0003 Plan is a first-class, previewable concept, ADR-0004 The core never branches on the source, ADR-0005 JII is never fully run as root, ADR-0006 Trust levels drive consent; default_yes is a threshold, ADR-0007 Expressive execution model: Action enum + plan executor (+55 more)

### Community 3 - "Config & Runtime Types"
Cohesion: 0.05
Nodes (34): Arc, AtomicBool, Default, Drop, Error, ExitCode, Into, JoinHandle (+26 more)

### Community 4 - "CLI Integration Tests"
Cohesion: 0.06
Nodes (45): arch_mismatch_is_flagged(), CacheAction, candidate(), candidate_line_includes_source_version_trust(), cargo_bin_check_is_skipped_when_irrelevant(), Commands, detect_system_manager(), dry_run_edit_file_never_writes() (+37 more)

### Community 5 - "Project Constraints & Handoff Docs"
Cohesion: 0.06
Nodes (52): AI Handoff Policy, Golden Rules, AGENTS.md Onboarding, Repository is the single source of truth, Core never branches on the source, Fedora-first Constraint, JSON State File (not SQLite yet), CLAUDE.md Project Instructions (+44 more)

### Community 6 - "Supported Systems & Release Testing"
Cohesion: 0.05
Nodes (44): Release Test Plan (manual), Packaging/install release-artifact checks, Pre-flight (build/clippy/test, version==tag), Trust barrier (untrusted always confirmed even with --auto), apt provider (Debian/Ubuntu), Cross-distro sources (flatpak/snap/brew/cargo/npm/pipx/go/github), dnf provider (Fedora tier 1), Supported Systems & Smoke Test (+36 more)

### Community 7 - "DNF Provider"
Cohesion: 0.11
Nodes (14): batch_merges_into_one_root_dnf_command(), batch_remove_merges_into_one_root_dnf_command(), batch_update_merges_into_one_root_dnf_command(), Dnf, missing_fields_are_tolerated(), parse_candidates(), parse_info(), parse_info_takes_first_stanza_and_folds_continuations() (+6 more)

### Community 8 - "Gentoo Provider"
Cohesion: 0.12
Nodes (16): argv_of(), atom(), batch_install_merges_atoms(), Block, Gentoo, install_uses_the_full_atom(), parse_search(), parses_atom_version_and_description() (+8 more)

### Community 9 - "Self-update & Asset Verification"
Cohesion: 0.15
Nodes (26): Verification, Asset, checksum_companion_is_matched_not_the_binary(), current_version(), deb_arch(), detect_install(), fetch_sha256(), Install (+18 more)

### Community 10 - "COPR Provider (search)"
Cohesion: 0.14
Nodes (24): HashMap, candidate(), candidate_is_community_with_project_in_raw(), fedora_chroots(), fedora_chroots_counts_matching_only(), parses_search_response(), Project, projects() (+16 more)

### Community 11 - "COPR Provider (planning)"
Cohesion: 0.11
Nodes (10): cargo_plan(), build_install_plan(), Copr, install_plan_is_enable_then_install_both_root(), root_plan(), go_install_plan(), install_plan_is_one_unprivileged_go_command(), snap_plan() (+2 more)

### Community 12 - "Forge Asset Classification"
Cohesion: 0.12
Nodes (20): arch_tokens(), asset(), asset_score(), AssetKind, candidate(), classify(), find_checksums_asset(), finds_and_parses_checksums() (+12 more)

### Community 13 - "APT Provider"
Cohesion: 0.13
Nodes (13): Apt, argv_of(), batch_install_merges_into_one_root_apt_command(), batch_remove_merges_into_one_root_apt_command(), batch_update_uses_only_upgrade(), cand(), ignores_description_md5_and_folded_body(), parse_show() (+5 more)

### Community 14 - "Homebrew Provider"
Cohesion: 0.12
Nodes (13): batch_merges_into_one_unprivileged_brew_command(), brew_many(), brew_plan(), candidate(), Formula, formula_becomes_a_community_candidate(), formula_without_stable_version_still_offered(), Homebrew (+5 more)

### Community 15 - "pipx Provider"
Cohesion: 0.11
Nodes (14): candidate(), install_and_upgrade_are_unprivileged_pipx_commands(), parse_pipx_list(), parses_pipx_list(), Pipx, pipx_plan(), PipxList, PipxMain (+6 more)

### Community 16 - "Void Provider"
Cohesion: 0.13
Nodes (13): argv_of(), batch_install_merges_into_one_root_command(), batch_remove_uses_recursive_flag(), install_plan_is_one_root_sync_install(), parse_query_list(), parse_show(), parses_exact_stanza(), parses_installed_list() (+5 more)

### Community 17 - "Cargo Provider"
Cohesion: 0.14
Nodes (13): batch_merges_into_one_unprivileged_cargo_command(), batch_remove_merges_into_one_unprivileged_cargo_uninstall(), binary_crate_becomes_a_community_candidate(), candidate(), Cargo, CrateInfo, CrateResponse, install_list_skips_blank_and_binary_lines() (+5 more)

### Community 18 - "Forge Install Planning"
Cohesion: 0.12
Nodes (10): bin_dir(), build_install_plan(), build_plan_downloads_verified_then_places_executable(), build_plan_extracts_from_archive(), build_plan_marks_unverified_without_checksum(), cache_dir(), ForgeProvider, is_placed() (+2 more)

### Community 19 - "Nix Config Editing"
Cohesion: 0.12
Nodes (18): exact_pname_wins_over_near_names(), find_list(), find_list_matches_the_system_attr_too(), insert_package(), insert_preserves_comments_and_lands_after_a_trailing_comment(), insert_works_without_a_with_pkgs_wrapper(), inserted(), Insertion (+10 more)

### Community 20 - "Snap Provider"
Cohesion: 0.16
Nodes (15): batch_install_merges_non_classic_but_declines_when_any_is_classic(), candidate(), Channel, ChannelEntry, classic_confinement_is_recorded(), classic_snap_install_adds_classic_flag(), install_plan_is_one_root_snap_command(), is_classic() (+7 more)

### Community 21 - "GitHub Forge & Repo Search"
Cohesion: 0.12
Nodes (13): Forge, Release, GhAsset, GhRelease, GhRepo, GhSearch, GithubForge, parses_and_normalizes_github_release_json() (+5 more)

### Community 22 - "Go Provider"
Cohesion: 0.13
Nodes (10): batch_merges_into_one_go_install_with_latest_suffixes(), binary_name(), candidate(), escape_module(), Go, go_bin_dir(), go_bin_display(), GoLatest (+2 more)

### Community 23 - "Pacman Provider"
Cohesion: 0.15
Nodes (10): argv_of(), batch_install_merges_into_one_root_pacman_command(), batch_remove_uses_recursive_flag(), Pacman, parse_query(), parse_si(), parses_first_stanza_with_url_value_intact(), parses_query_name_and_version() (+2 more)

### Community 24 - "Zypper Provider"
Cohesion: 0.14
Nodes (9): argv_of(), attr(), batch_install_is_one_non_interactive_root_command(), batch_update_uses_update_verb(), parse_search_xml(), parses_first_solvable_skipping_the_list_container(), rec(), root_plan() (+1 more)

### Community 25 - "AUR Provider"
Cohesion: 0.17
Nodes (9): Aur, aur_helper(), candidate(), candidate_carries_community_trust_and_unsigned(), helper_plan(), parse_qm(), parses_qm_lines(), RpcPackage (+1 more)

### Community 26 - "Engine Core (search/rank/bootstrap)"
Cohesion: 0.13
Nodes (6): typo_variants_recover_common_slips(), EcosystemStatus, SearchResult, typo_variants(), Bootstrap, PackageCandidate

### Community 27 - "Registry (JSON state)"
Cohesion: 0.17
Nodes (8): Action, install_then_get(), record(), Registry, reinstall_replaces_not_duplicates(), remove_clears_record_and_logs(), roundtrips_through_json(), update_refreshes_version_and_logs_as_update()

### Community 28 - "Provider Trait & Registry"
Cohesion: 0.15
Nodes (14): B, Box, Item, Iterator, get_json_opt(), nonempty_lines(), parse_installed_records(), parses_installed_records() (+6 more)

### Community 29 - "Ranking Logic"
Cohesion: 0.15
Nodes (15): GlobalArgs, among_prefix_matches_the_shorter_name_is_closer(), candidate(), drops_arch_incompatible_candidates(), effective_rank(), exact_appid_tail_outranks_an_unrelated_same_named_package(), exact_name_match_outranks_a_higher_priority_prefix_match(), name_match_tier() (+7 more)

### Community 30 - "Package Model & Spec Grammar"
Cohesion: 0.11
Nodes (6): Action, MatchMode, PackageSpec, Query, QueryKind, spec()

### Community 31 - "Recommend Catalog"
Cohesion: 0.18
Nodes (10): HashSet, Catalog, distro_filter_selects_matching_entries(), embedded_catalog_parses(), entry(), entry_titles_are_unique(), prerequisite(), prerequisite_fires_only_when_needed() (+2 more)

### Community 32 - "Search Cache"
Cohesion: 0.20
Nodes (8): Mutex, PathBuf, a_zero_cooldown_never_circuit_breaks(), Cache, empty(), failure_opens_and_success_closes_the_circuit(), failures_are_per_source(), key()

### Community 33 - "Flatpak Provider"
Cohesion: 0.22
Nodes (15): batch_update_merges_into_one_unprivileged_flatpak_command(), best_match(), best_match_none_when_unrelated(), best_match_prefers_the_app_over_plugins_and_manual(), candidate_from(), candidate_uses_appid_as_name_and_prefers_flathub(), choose_remote(), flathub_search() (+7 more)

### Community 34 - "Platform Detection"
Cohesion: 0.14
Nodes (8): detect_path_dirs(), detect_tty(), detect_unicode(), Distro, ElevationKind, parse_arch_like(), parse_distro(), Platform

### Community 35 - "Provider Catalog (concepts)"
Cohesion: 0.11
Nodes (19): No Core Branch on Source (ADR-0004), Optional Provider Capabilities (ADR-0022 growth), AUR Provider, Gentoo (Portage/emerge) Provider, Void (XBPS) Provider, --run flag / Provider::launch_command, Homebrew Provider, Cargo Provider (+11 more)

### Community 36 - "Audit & Verification"
Cohesion: 0.19
Nodes (11): audit_concerns(), AuditConcern, AuditEntry, AuditVerification, disabled_source_is_flagged(), official_verified_install_has_no_concerns(), plan_verification(), resolve_verification() (+3 more)

### Community 37 - "npm Provider"
Cohesion: 0.18
Nodes (6): install_plan_is_one_unprivileged_user_prefixed_command(), Npm, npm_plan(), parse_ls_json(), parses_ls_json(), user_prefix()

### Community 38 - "Charts Generation Script"
Cohesion: 0.20
Nodes (17): DateTime, build_chart(), esc(), fetch_downloads(), fetch_starred_at(), gh_get(), main(), nice_step() (+9 more)

### Community 39 - "Provider Info & Ecosystem"
Cohesion: 0.21
Nodes (4): Option, Ecosystem, PackageInfo, Reference

### Community 40 - "Nix Provider Commands"
Cohesion: 0.25
Nodes (5): command_plan(), run_capture_lax(), flake_ref(), Nix, nix_argv()

### Community 41 - "npm Manifest Parsing"
Cohesion: 0.23
Nodes (11): bin_as_string_is_a_cli(), candidate(), cli_package_becomes_a_community_candidate(), has_bin(), library_note(), library_note_is_actionable(), library_only_package_yields_no_candidate(), LsDep (+3 more)

### Community 42 - "Plan JSON Serialization"
Cohesion: 0.18
Nodes (13): Action, element_name(), first_store_basename(), parse_profile_list(), parses_modern_map_schema_with_names_and_versions(), parses_older_array_schema_deriving_name_from_attrpath(), record(), store_name() (+5 more)

### Community 43 - "UI Palette"
Cohesion: 0.23
Nodes (5): candidate_line(), repo_label(), String, Palette, menu_line()

### Community 44 - "Declarative Install Strategies"
Cohesion: 0.20
Nodes (12): F, candidate_targets(), detect_targets(), detect_targets_offers_only_existing_config_files(), detect_targets_with(), guidance(), guidance_shows_file_snippet_and_apply_command_but_no_write(), NixTarget (+4 more)

### Community 45 - "Installed Resolution & Batch"
Cohesion: 0.29
Nodes (3): npm_many(), snap_many(), InstalledRecord

### Community 47 - "HTTP Client & Probe"
Cohesion: 0.24
Nodes (5): Client, fetch_text(), fetch_rate_limit(), http_client(), Probe

### Community 48 - "Source Catalog & Trust"
Cohesion: 0.20
Nodes (3): source_relevant(), SourceEntry, TrustLevel

### Community 49 - "install.sh Script"
Cohesion: 0.49
Nodes (8): install.sh script, ask_default_yes(), dl(), err(), info(), native_install(), portable_install(), verify_sha256()

### Community 50 - "Nix Batch & Diff"
Cohesion: 0.22
Nodes (7): argv_of(), batch_install_merges_flake_refs(), line_diff(), line_diff_shows_the_single_added_line_with_context(), nix_profile_bin(), rec(), update_uses_profile_upgrade()

### Community 51 - "Search & GitHub Features"
Cohesion: 0.22
Nodes (9): Per-source Failure Circuit Breaker, GitHub Repository Chooser / By-name Search, Smart Search Matching (broaden_search + typo tolerance), AppImage (delivery format over GitHub), Search Cache (TTL + stale-on-error), Engine (search-rank-plan-execute), Forge Abstraction (ForgeProvider), GitHub Releases Provider (+1 more)

### Community 53 - "Bootstrap & Setup Features"
Cohesion: 0.25
Nodes (8): Bootstrap Missing Managers (T6), can_search Capability, Cooperation Principle (not the centre of the world), jii sources (disable/enable/add/remove), jii doctor (system helper), First-run Wizard / jii setup, Recommend Catalog (data-driven, distro-aware), Actionable Errors (JiiError::remedy)

### Community 54 - "Source Health / doctor"
Cohesion: 0.32
Nodes (4): Duration, health_from(), SourceHealth, Health

### Community 55 - "Record Batching"
Cohesion: 0.33
Nodes (6): group_by_source(), groups_by_source_preserving_first_seen_order(), plan_one_record(), RecordBatch, RecordOp, Fn

### Community 56 - "Plan & Execution Concepts"
Cohesion: 0.33
Nodes (6): Batch Install (plan_install_many), Declarative Nix (install_strategies / EditFile), Action Enum + Executor (exec.rs), InstallPlan, privilege.rs (batched escalation), Nix Provider

### Community 57 - "PkgVersion Type"
Cohesion: 0.40
Nodes (4): version_or_unknown(), Display, Formatter, PkgVersion

### Community 58 - "Candidate Selection Model"
Cohesion: 0.40
Nodes (5): Candidate Chooser (ui/prompt::choose), PackageCandidate, Ranking (priority + trust tie-breaker), Trust Levels (official/community/untrusted), PackageSpec Grammar name[:source][@ref]

### Community 59 - "Brand Assets"
Cohesion: 1.00
Nodes (3): JII Banner (Just Install It), JII Icon (Isometric Cube Logo), JII Social Preview Card

### Community 60 - "Project State Docs"
Cohesion: 0.67
Nodes (3): AI Context (Current State Snapshot), JII Tasks Checklist, Terminal 1.0 UX Evaluation

### Community 61 - "UI Progress & Output"
Cohesion: 0.67
Nodes (3): Semantic Palette + Unicode/TTY Fallback, ui::Spinner (live progress), Progressive/Streaming Search

### Community 63 - "Charts Workflow"
Cohesion: 0.67
Nodes (3): scripts/gen_charts.py, charts render job, Charts workflow

## Ambiguous Edges - Review These
- `Default-method pattern (ADR-0022)` → `default_yes as trust threshold`  [AMBIGUOUS]
  docs/ARCHITECTURE.md · relation: conceptually_related_to

## Knowledge Gaps
- **64 isolated node(s):** `SourcesAction`, `Working Style`, `Static musl binary (x86_64 / aarch64)`, `Nix declarative config editing`, `Package spec syntax name[:source]` (+59 more)
  These have ≤1 connection - possible missing edges or undocumented components.
- **17 thin communities (<3 nodes) omitted from report** — run `graphify query` to explore isolated nodes.

## Suggested Questions
_Questions this graph is uniquely positioned to answer:_

- **What is the exact relationship between `Default-method pattern (ADR-0022)` and `default_yes as trust threshold`?**
  _Edge tagged AMBIGUOUS (relation: conceptually_related_to) - confidence is low._
- **Why does `PackageCandidate` connect `Engine Core (search/rank/bootstrap)` to `CLI Command Dispatch`, `CLI Integration Tests`, `DNF Provider`, `Gentoo Provider`, `COPR Provider (search)`, `COPR Provider (planning)`, `Forge Asset Classification`, `APT Provider`, `Homebrew Provider`, `pipx Provider`, `Void Provider`, `Cargo Provider`, `Forge Install Planning`, `Nix Config Editing`, `Snap Provider`, `Go Provider`, `Pacman Provider`, `Zypper Provider`, `AUR Provider`, `Ranking Logic`, `Package Model & Spec Grammar`, `Search Cache`, `Flatpak Provider`, `npm Provider`, `Charts Generation Script`, `Provider Info & Ecosystem`, `Nix Provider Commands`, `npm Manifest Parsing`, `Plan JSON Serialization`, `UI Palette`, `Declarative Install Strategies`, `Flatpak Plans`, `HTTP Client & Probe`, `Source Catalog & Trust`, `Source Health / doctor`, `PkgVersion Type`?**
  _High betweenness centrality (0.052) - this node is a cross-community bridge._
- **Why does `InstallPlan` connect `COPR Provider (planning)` to `CLI Command Dispatch`, `Privileged Exec & Archives`, `DNF Provider`, `Gentoo Provider`, `Self-update & Asset Verification`, `APT Provider`, `Homebrew Provider`, `pipx Provider`, `Void Provider`, `Cargo Provider`, `Forge Install Planning`, `Snap Provider`, `Go Provider`, `Pacman Provider`, `Zypper Provider`, `AUR Provider`, `Package Model & Spec Grammar`, `Audit & Verification`, `npm Provider`, `Provider Info & Ecosystem`, `Nix Provider Commands`, `Plan JSON Serialization`, `UI Palette`, `Installed Resolution & Batch`, `Flatpak Plans`, `Nix Batch & Diff`, `Record Batching`?**
  _High betweenness centrality (0.042) - this node is a cross-community bridge._
- **Why does `InstalledRecord` connect `Installed Resolution & Batch` to `CLI Command Dispatch`, `DNF Provider`, `Gentoo Provider`, `COPR Provider (planning)`, `APT Provider`, `Homebrew Provider`, `pipx Provider`, `Void Provider`, `Cargo Provider`, `Forge Install Planning`, `Snap Provider`, `Go Provider`, `Pacman Provider`, `Zypper Provider`, `AUR Provider`, `Registry (JSON state)`, `Provider Trait & Registry`, `Package Model & Spec Grammar`, `Audit & Verification`, `npm Provider`, `Charts Generation Script`, `Provider Info & Ecosystem`, `Nix Provider Commands`, `Plan JSON Serialization`, `UI Palette`, `Flatpak Plans`, `Nix Batch & Diff`, `Record Batching`, `PkgVersion Type`?**
  _High betweenness centrality (0.029) - this node is a cross-community bridge._
- **What connects `All star timestamps, ascending. Paginates 100 at a time.`, `Cumulative download totals over release publish dates.      GitHub exposes only`, `A round-ish integer step so an axis reads 0, s, 2s, …` to the rest of the system?**
  _98 weakly-connected nodes found - possible documentation gaps or missing edges._
- **Should `CLI Command Dispatch` be split into smaller, more focused modules?**
  _Cohesion score 0.07058650162098438 - nodes in this community are weakly interconnected._
- **Should `Privileged Exec & Archives` be split into smaller, more focused modules?**
  _Cohesion score 0.06013986013986014 - nodes in this community are weakly interconnected._