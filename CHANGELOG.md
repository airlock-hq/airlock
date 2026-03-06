# Changelog

## [0.1.53](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.52...airlock-v0.1.53) (2026-03-06)


### Bug Fixes

* **defaults:** replace jq with airlock exec json ([#33](https://github.com/airlock-hq/airlock/issues/33)) ([53383e8](https://github.com/airlock-hq/airlock/commit/53383e8aa153ca1e28a2a710547df0a8ea3ebce6))

## [0.1.52](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.51...airlock-v0.1.52) (2026-03-06)


### Features

* show mermaid diagram in PR ([#32](https://github.com/airlock-hq/airlock/issues/32)) ([956f7fb](https://github.com/airlock-hq/airlock/commit/956f7fb6de32a6f98f5c0ad5467114d27ee357fc))


### Bug Fixes

* show airlock banner only if push matched a pipeline ([#30](https://github.com/airlock-hq/airlock/issues/30)) ([3e66ca3](https://github.com/airlock-hq/airlock/commit/3e66ca3efce65b855f88a80b38f299398f37c9ae))

## [0.1.51](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.50...airlock-v0.1.51) (2026-03-06)


### Features

* show error when step has no stdout/stderr ([dc5de0c](https://github.com/airlock-hq/airlock/commit/dc5de0c01800dd772508d684f2b067b2863fa73f))


### Bug Fixes

* **daemon:** make run queue cancellation ref-aware ([#28](https://github.com/airlock-hq/airlock/issues/28)) ([60ef542](https://github.com/airlock-hq/airlock/commit/60ef542710b37c4888bb33a4c2014cf8f4c46157))
* record correct branch sha in rebase ([72a7a51](https://github.com/airlock-hq/airlock/commit/72a7a51e57a381485ce1c129e71b5ad83e33c1dd))
* **window-resize:** smooth out window launch ([#29](https://github.com/airlock-hq/airlock/issues/29)) ([93c2ec9](https://github.com/airlock-hq/airlock/commit/93c2ec97c8b8214c25b137a7f754f9aae664d67f))
* **window:** unminimize window on show ([b3a79dc](https://github.com/airlock-hq/airlock/commit/b3a79dc8d92b34ce98aec3d74ae4d0104c944160))

## [0.1.50](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.49...airlock-v0.1.50) (2026-03-05)


### Features

* **agent:** timeout and kill stalled streams ([#24](https://github.com/airlock-hq/airlock/issues/24)) ([90bdfe3](https://github.com/airlock-hq/airlock/commit/90bdfe3a27fc32c63bb30f689ccdf9cb6fbf25a2))

## [0.1.49](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.48...airlock-v0.1.49) (2026-03-05)


### Bug Fixes

* better catching of rebase state errors in push stage ([05c6bea](https://github.com/airlock-hq/airlock/commit/05c6bea3ee861a6752c061667dd3479643721062))
* **core:** resolve push lease handling ([4d4c645](https://github.com/airlock-hq/airlock/commit/4d4c645676f6fd932e13333e82f4be7559bb19e3))
* handle process_coalesced_push return changes ([f32fdd7](https://github.com/airlock-hq/airlock/commit/f32fdd7ed6ce71b918f2517ee5d8ed45ca963aef))
* **push:** cancel active run when superseded ([faf1c09](https://github.com/airlock-hq/airlock/commit/faf1c098c19502e783176a91b9befbac92c7d8ad))
* **push:** filter workflows by branch and forward unmatched branches ([938508c](https://github.com/airlock-hq/airlock/commit/938508cd5f5b84516b7ca57e6f25abdc947a0c61))
* **push:** forward unmatched branch refs to upstream ([3545b24](https://github.com/airlock-hq/airlock/commit/3545b243a013b160e2ca671481459736a61e5eae))
* **push:** handle unmatched branches correctly ([39a9925](https://github.com/airlock-hq/airlock/commit/39a9925d484834bedf2d15abcc47420bd6437b7a))
* **push:** improve branch workflow handling ([959743f](https://github.com/airlock-hq/airlock/commit/959743f9bee2550a34a9406b235bd4f6ee8f6550))

## [0.1.48](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.47...airlock-v0.1.48) (2026-03-04)


### Bug Fixes

* clean CLAUDE env vars on daemon startup ([#19](https://github.com/airlock-hq/airlock/issues/19)) ([b677883](https://github.com/airlock-hq/airlock/commit/b677883cbed61ec45ebeabfa049f0a03f3c59f4a))

## [0.1.47](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.46...airlock-v0.1.47) (2026-03-02)


### Bug Fixes

* reset worktree handling and improve pipeline logic ([c7805f2](https://github.com/airlock-hq/airlock/commit/c7805f22b3d2d8aaa46f8111ebda1ca1daeb61ea))

## [0.1.46](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.45...airlock-v0.1.46) (2026-03-02)


### Bug Fixes

* **run-detail:** show copied state for copy comments button ([fc1a290](https://github.com/airlock-hq/airlock/commit/fc1a290f471e5d11ad09f3b5b7b1c29fd87d4bda))

## [0.1.45](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.44...airlock-v0.1.45) (2026-03-01)


### Features

* **activity-feed:** add activity feed UI ([554fe37](https://github.com/airlock-hq/airlock/commit/554fe3751a2a33534fd6f30b07da337b3e60a332))
* add critique comment UI and patch selection handling ([a7098b3](https://github.com/airlock-hq/airlock/commit/a7098b358dc6db7925176046039d309cc9bb261b))
* **events:** more notifications ([368de9b](https://github.com/airlock-hq/airlock/commit/368de9bd9a5d39f76ee40025be491c81e141a1bc))


### Bug Fixes

* better window state ([88daf73](https://github.com/airlock-hq/airlock/commit/88daf733cc22664e7738891aa684efa93204a02b))
* buf fixes for edge cases ([8621c15](https://github.com/airlock-hq/airlock/commit/8621c155adec485a649851752fe4b84245222bb2))
* centralize comment and patch key generation ([57fbd2c](https://github.com/airlock-hq/airlock/commit/57fbd2c24d612f1d3357f48bc6eda38a0e0da542))
* ensure unique keys for steps and artifacts ([580df10](https://github.com/airlock-hq/airlock/commit/580df102a8347f45ce79984df7f4b2ef494d30b3))
* handle duplicate step names in approval flow ([85a205d](https://github.com/airlock-hq/airlock/commit/85a205d04c4024a22ece163730162ebda446a209))
* remove critique content artifact ([26fcd4b](https://github.com/airlock-hq/airlock/commit/26fcd4b565f6f7ff7e7bab556a9a11fcd2f20548))
* remove error label from run list ([008dcc5](https://github.com/airlock-hq/airlock/commit/008dcc56fbaca2e53721c573eb5a432c05d8ed64))
* resolve issue with duplicate step names ([65cae7d](https://github.com/airlock-hq/airlock/commit/65cae7d0e4808b31b6d3941a30e29ba1f03d3376))
* **steps:** handle duplicate step names in approval flow ([873a29e](https://github.com/airlock-hq/airlock/commit/873a29ede266ffc8e0e1dd09407e6c016ca9fd63))

## [0.1.44](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.43...airlock-v0.1.44) (2026-02-28)


### Features

* allow re-init of Airlock ([492bd07](https://github.com/airlock-hq/airlock/commit/492bd07c91b8f7e22f310a743af8d05ade776121))
* **init:** add approval mode handling and allow re-init ([5d69c39](https://github.com/airlock-hq/airlock/commit/5d69c39d7a783b6f806b0f90c5c7fd29ca126b25))
* **notification:** add OS push notifications and tray icon ([92f14a9](https://github.com/airlock-hq/airlock/commit/92f14a91473a262d05614ad986d476fd1be516bc))
* rename Changes tab to Critique and show count ([37394a7](https://github.com/airlock-hq/airlock/commit/37394a7809f4d8b2264fc01a5b83651828f1704c))

## [0.1.43](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.42...airlock-v0.1.43) (2026-02-28)


### Features

* **critique:** add critique step to workflow ([7daa6ad](https://github.com/airlock-hq/airlock/commit/7daa6ada4b2dd2a049d448bc765281224713ee7a))
* **rebase:** add dual-phase upstream sync and conflict resolution ([e6d436e](https://github.com/airlock-hq/airlock/commit/e6d436e1426a6670baf68868e514b2d72f46f294))


### Bug Fixes

* approval flow ([3e729b2](https://github.com/airlock-hq/airlock/commit/3e729b26e4100eb13d8292224ab700fed23beac0))
* **approval:** add pre-execution pause for IfPatches ([83e8aec](https://github.com/airlock-hq/airlock/commit/83e8aec50b0b5694854cd00eb17c565c3214a691))

## [0.1.42](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.41...airlock-v0.1.42) (2026-02-27)


### Bug Fixes

* adjust diagram zoom control ([54fb65e](https://github.com/airlock-hq/airlock/commit/54fb65ef62c290246d7cd9c8a21ff2df4604a1ed))
* mermaid diagram crash ([12655fc](https://github.com/airlock-hq/airlock/commit/12655fc046cd777f78997d2286b4afcd8a597533))
* mermaid diagram scroll direction ([daad974](https://github.com/airlock-hq/airlock/commit/daad974c3a2d25ba4374faf9cd42a6de1cf2b5b2))

## [0.1.41](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.40...airlock-v0.1.41) (2026-02-27)


### Features

* **component:** add mermaid diagram support ([f5cb553](https://github.com/airlock-hq/airlock/commit/f5cb553eb84f49bf836cb9b0f6fd2925467b32f9))
* mermaid diagram in describe ([6e067f4](https://github.com/airlock-hq/airlock/commit/6e067f42fdfc33a4368dc09ecf2b952ca5ec9f52))


### Bug Fixes

* cleanup push marker refs ([4ac097f](https://github.com/airlock-hq/airlock/commit/4ac097f711124f5a4a9e9abb1bd072129b0c9730))
* make diagram zoom to fit ([5580e83](https://github.com/airlock-hq/airlock/commit/5580e83a35e3830a6dab57451838f0bb4012d93b))
* no need for content artifact for some steps ([fe726d8](https://github.com/airlock-hq/airlock/commit/fe726d8db93734ab48ca95a7a30aa5319e578551))
* remove lint content artifact ([3eebe28](https://github.com/airlock-hq/airlock/commit/3eebe289484c61c7b567823268a44bb2dc6e0087))

## [0.1.40](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.39...airlock-v0.1.40) (2026-02-27)


### Features

* **tabs:** add content count badge and reorder tabs ([3317b48](https://github.com/airlock-hq/airlock/commit/3317b489eae80db34a915583b8a9d150cc6edce0))


### Bug Fixes

* **handlers:** improve patch apply error handling ([74b9103](https://github.com/airlock-hq/airlock/commit/74b910355baa8bf2a7ef144dd76f10b9249ea412))

## [0.1.39](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.38...airlock-v0.1.39) (2026-02-27)


### Bug Fixes

* **cli:** ux improvements ([6fe36c6](https://github.com/airlock-hq/airlock/commit/6fe36c6d70b97bfcfe3369d8b89360a2a0de993f))
* improve shell PATH resolution ([fc82cb2](https://github.com/airlock-hq/airlock/commit/fc82cb2816f8974eb269513fc3de6ec2d2a547a1))

## [0.1.38](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.37...airlock-v0.1.38) (2026-02-25)


### Bug Fixes

* **workflow:** rename require_approval flag ([e64c9da](https://github.com/airlock-hq/airlock/commit/e64c9dabcf4f3e08d7a18c8176bf24d0c5d0e25a))

## [0.1.37](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.36...airlock-v0.1.37) (2026-02-25)


### Bug Fixes

* more robust patch apply behavior ([d1657f2](https://github.com/airlock-hq/airlock/commit/d1657f20eb01a405970a081deb88ff7043e77b85))

## [0.1.36](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.35...airlock-v0.1.36) (2026-02-24)


### Features

* initial commit ([cef9745](https://github.com/airlock-hq/airlock/commit/cef9745ecf0d7b47d9d2383a759e161878c43dad))

## [0.1.35](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.34...airlock-v0.1.35) (2026-02-24)


### Features

* initial commit ([cef9745](https://github.com/airlock-hq/airlock/commit/cef9745ecf0d7b47d9d2383a759e161878c43dad))

## [0.1.34](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.33...airlock-v0.1.34) (2026-02-24)


### Features

* initial commit ([d0aebec](https://github.com/airlock-hq/airlock/commit/d0aebec3636b7b969d8e9d23d40ce3b7c1833575))

## [0.1.33](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.32...airlock-v0.1.33) (2026-02-24)


### Bug Fixes

* allow codex agent to run builds ([8277905](https://github.com/airlock-hq/airlock/commit/82779052e6fd00b83289b506557617ebd115d0f8))
* clean up stale persistent worktree ([9a99abf](https://github.com/airlock-hq/airlock/commit/9a99abfdd6298039065209aa3b9f02934095d10a))

## [0.1.32](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.31...airlock-v0.1.32) (2026-02-24)


### Features

* umami anonymous analytics ([30e78d6](https://github.com/airlock-hq/airlock/commit/30e78d632c9fe20c45c7abbb646e796da5bdd5bb))

## [0.1.31](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.30...airlock-v0.1.31) (2026-02-23)


### Bug Fixes

* trigger release ([620552d](https://github.com/airlock-hq/airlock/commit/620552d14935dfcc9c489f46812c469d111002bd))

## [0.1.30](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.29...airlock-v0.1.30) (2026-02-23)


### Bug Fixes

* edge case of stale gate repo worktree ([a2b6098](https://github.com/airlock-hq/airlock/commit/a2b60987c5955e0c25fd2a38b5e1a0f084c64c55))

## [0.1.29](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.28...airlock-v0.1.29) (2026-02-22)


### Features

* **diff:** add per-commit diff support ([4b1a03c](https://github.com/airlock-hq/airlock/commit/4b1a03c04480d4cfae7209733356fe51deb071e9))


### Bug Fixes

* **config:** add agent config fields ([815dda7](https://github.com/airlock-hq/airlock/commit/815dda7dbb8f85a72920f2e4a21c500c8232b233))
* normalize required schema for codex ([ee20fbe](https://github.com/airlock-hq/airlock/commit/ee20fbe7bfaf4a14ae64b67a6f7377ea351a0c9f))

## [0.1.28](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.27...airlock-v0.1.28) (2026-02-22)


### Bug Fixes

* claude code adapter ([b604407](https://github.com/airlock-hq/airlock/commit/b604407907c7af246af128deb7cb5b37fc0b01ca))
* codex adapter schema error ([b599112](https://github.com/airlock-hq/airlock/commit/b59911205890b70891fa2e82e8e50dce17b4d9d4))
* truncate large log output ([ff5901f](https://github.com/airlock-hq/airlock/commit/ff5901fc20e2b32c996f9ea4dcdd054b0131f063))

## [0.1.27](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.26...airlock-v0.1.27) (2026-02-22)


### Bug Fixes

* **codex:** ensure stream terminates after EOF ([7fca890](https://github.com/airlock-hq/airlock/commit/7fca890b18bc3447f9eaa85308d4d596c9f19e7d))

## [0.1.26](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.25...airlock-v0.1.26) (2026-02-22)


### Features

* **document:** add documentation step ([e767b5a](https://github.com/airlock-hq/airlock/commit/e767b5a9d6380803e37e5ad2efa9915b492ae689))

## [0.1.25](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.24...airlock-v0.1.25) (2026-02-22)


### Bug Fixes

* adjust orbital line positions ([906b984](https://github.com/airlock-hq/airlock/commit/906b98415a55e8c6d078b932ce29f1a3f4f790d0))

## [0.1.24](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.23...airlock-v0.1.24) (2026-02-22)


### Features

* split react entry ([170bac8](https://github.com/airlock-hq/airlock/commit/170bac851a6d6a186dd02a88aabf2d7ec85ff657))


### Bug Fixes

* handle unknown claude code events ([4d239af](https://github.com/airlock-hq/airlock/commit/4d239af6fc4a13f27ad06dcdae3341eb7f6592f2))

## [0.1.23](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.22...airlock-v0.1.23) (2026-02-22)


### Features

* add resizable panels to design system ([e4b4703](https://github.com/airlock-hq/airlock/commit/e4b4703bbf98da5173a326afcf503cfdc1fec1e9))
* detect SCM provider ([0e35af2](https://github.com/airlock-hq/airlock/commit/0e35af2d334d1d0fe1f12d9d5f847ec68f5945db))


### Bug Fixes

* auto forward during push ([b36a85e](https://github.com/airlock-hq/airlock/commit/b36a85ebf314319c28d5ff71de6eb9d60fadfeb2))
* fix test failures ([1ad180b](https://github.com/airlock-hq/airlock/commit/1ad180ba4d1269e7b80e5ee3c53571ea278929eb))
* improve default steps ([9932f70](https://github.com/airlock-hq/airlock/commit/9932f7021ea458b4831940f8e48b648be14abda1))
* simplify daemon install flow ([0eb4bd3](https://github.com/airlock-hq/airlock/commit/0eb4bd3fa38b5f870bacda749bdb878c002509ba))
* update ascii banner ([865d844](https://github.com/airlock-hq/airlock/commit/865d844f6d258eeb11950f5b932f2d092368ec05))

## [0.1.22](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.21...airlock-v0.1.22) (2026-02-22)


### Features

* **daemon:** add upstream base SHA handling for push ([c553cee](https://github.com/airlock-hq/airlock/commit/c553ceed3b0f7ca168f0ece0891cf142c7625e35))

## [0.1.21](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.20...airlock-v0.1.21) (2026-02-22)


### Bug Fixes

* fix push result artifact display ([8fd9a7c](https://github.com/airlock-hq/airlock/commit/8fd9a7cb4c715d69297fb48b76920e9444850323))
* improve lint and test step scripts ([517c115](https://github.com/airlock-hq/airlock/commit/517c115d02c3ac878a99e53c7bfd5d380f20db79))
* **pipeline:** add log streaming to disk ([22f9c91](https://github.com/airlock-hq/airlock/commit/22f9c91cad191867b0b66d9560a9b6be3a2ef3e1))

## [0.1.20](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.19...airlock-v0.1.20) (2026-02-22)


### Bug Fixes

* update build scripts and service loading ([c19537d](https://github.com/airlock-hq/airlock/commit/c19537de42cc905546b4e24f373bc7401b018a06))

## [0.1.19](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.18...airlock-v0.1.19) (2026-02-22)


### Bug Fixes

* show existing stdout/stderr content ([b23d585](https://github.com/airlock-hq/airlock/commit/b23d585332fcadd5f1c00b8f94059b129f53adfc))

## [0.1.18](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.17...airlock-v0.1.18) (2026-02-22)


### Features

* **core:** add smart sync support ([c2c9bb9](https://github.com/airlock-hq/airlock/commit/c2c9bb919f5a591c408b07a875a59976cf28f17b))

## [0.1.17](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.16...airlock-v0.1.17) (2026-02-22)


### Features

* agent card in settings page ([3ae1ba6](https://github.com/airlock-hq/airlock/commit/3ae1ba6257f2ef47c6c1c15078fd5fcf70563fea))
* **core:** add persistent worktree support ([76d459b](https://github.com/airlock-hq/airlock/commit/76d459b045c04fb0563b5b066d6b0f828c07bb5c))


### Bug Fixes

* improve homebrew upgrade experience ([e0b9f76](https://github.com/airlock-hq/airlock/commit/e0b9f765ae7c515e016ae933d58421812f95f241))
* simplify ipc client handling in tauri backend ([bf1434d](https://github.com/airlock-hq/airlock/commit/bf1434dbef0b86350538c3ac1d8d6a6e891d8478))
* streamline tool event mapping in claude agent ([c645183](https://github.com/airlock-hq/airlock/commit/c645183668b068dd18ed3ffb387f275afe6eb70c))

## [0.1.16](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.15...airlock-v0.1.16) (2026-02-21)


### Features

* revamp push request UI ([d9ef877](https://github.com/airlock-hq/airlock/commit/d9ef877a97bda707dabc8a709bdc8184f15e7917))

## [0.1.15](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.14...airlock-v0.1.15) (2026-02-21)


### Features

* **tauri:** add single-instance plugin and focus window on launch ([08091d0](https://github.com/airlock-hq/airlock/commit/08091d08e12c7e61363dea1f2e89e57c5052bc2b))


### Bug Fixes

* adjust orbital line positions in atmosphere component ([b34edfd](https://github.com/airlock-hq/airlock/commit/b34edfde0cafd853eff2d96db420e1bc298a04f3))

## [0.1.14](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.13...airlock-v0.1.14) (2026-02-21)


### Features

* allow timeout setting on each step ([55bae57](https://github.com/airlock-hq/airlock/commit/55bae57e4cc5dea05e2ee2aa89859de943e80535))
* **core:** add brand color constant and apply to CLI and hooks ([b326996](https://github.com/airlock-hq/airlock/commit/b326996600e1bee2b029b9870014edeb78ba9f7e))

## [0.1.13](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.12...airlock-v0.1.13) (2026-02-21)


### Bug Fixes

* optimize atmosphere animation performance ([10685e6](https://github.com/airlock-hq/airlock/commit/10685e6ee777f9d45d46f9ac9d2082f3435740ee))

## [0.1.12](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.11...airlock-v0.1.12) (2026-02-21)


### Features

* init wizard ([a542222](https://github.com/airlock-hq/airlock/commit/a542222082fd30dca5e787b157af9a5664324749))
* **patches:** apply selected patches ([1ba41b5](https://github.com/airlock-hq/airlock/commit/1ba41b594629173d4b62e570ed654d2d07ffe13e))


### Bug Fixes

* improve Patches tab UI ([16fa534](https://github.com/airlock-hq/airlock/commit/16fa53483afc180ffb9f775a5f9366bb84cdfa22))
* more robust login shell detection ([d036c79](https://github.com/airlock-hq/airlock/commit/d036c79b36341424267e81e462b39287291654ff))
* patches tab should distinguish applied vs pending patches ([e274c3c](https://github.com/airlock-hq/airlock/commit/e274c3c54442c77b97485a55514e36e7119e5474))

## [0.1.11](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.10...airlock-v0.1.11) (2026-02-21)


### Bug Fixes

* **service:** remove PATH injection from service templates ([d1e45e6](https://github.com/airlock-hq/airlock/commit/d1e45e6b7a4364e32a55573d6828e34d990020fe))

## [0.1.10](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.9...airlock-v0.1.10) (2026-02-21)


### Bug Fixes

* use login shell as default for steps ([736e8e4](https://github.com/airlock-hq/airlock/commit/736e8e4ffbb1fe2e820f4e5b725bb15e7d04e329))

## [0.1.9](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.8...airlock-v0.1.9) (2026-02-21)


### Features

* **daemon:** enhance PATH handling with user login shell ([a4df436](https://github.com/airlock-hq/airlock/commit/a4df436da9add5d081e88291c8aee1f4b4df8d08))

## [0.1.8](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.7...airlock-v0.1.8) (2026-02-21)


### Bug Fixes

* more graceful daemon stop handling ([f8216d4](https://github.com/airlock-hq/airlock/commit/f8216d483a17443d10a27ae5fed27e17d8400da7))

## [0.1.7](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.6...airlock-v0.1.7) (2026-02-21)


### Bug Fixes

* fixed position for atmosphere and job key in step log path ([#15](https://github.com/airlock-hq/airlock/issues/15)) ([48a895d](https://github.com/airlock-hq/airlock/commit/48a895d04b5c460fb349c519c86f60d5d645f4b7))

## [0.1.6](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.5...airlock-v0.1.6) (2026-02-20)


### Features

* use built-in step definitions if possible ([aae2b39](https://github.com/airlock-hq/airlock/commit/aae2b39e9a59ac95cf8e7eac362b601bd6acd112))


### Bug Fixes

* **design-system:** improve design system class scanning ([ff93fc9](https://github.com/airlock-hq/airlock/commit/ff93fc99ab9a7e8cd0fb4c812f9e0c751237ab12))

## [0.1.5](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.4...airlock-v0.1.5) (2026-02-20)


### Bug Fixes

* tweak atmosphere ([77e784c](https://github.com/airlock-hq/airlock/commit/77e784cc243ef243462c731ae39864b915bbcec0))

## [0.1.4](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.3...airlock-v0.1.4) (2026-02-20)


### Bug Fixes

* add design system and frontend build steps to release workflow ([e3215c9](https://github.com/airlock-hq/airlock/commit/e3215c975579d011b44ca9c2f5d393f7e47b455e))

## [0.1.3](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.2...airlock-v0.1.3) (2026-02-20)


### Bug Fixes

* add push command and gate sync ([61754eb](https://github.com/airlock-hq/airlock/commit/61754eb5bbc91c3aa9756d823ebec42e8346a859))

## [0.1.2](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.1...airlock-v0.1.2) (2026-02-19)


### Features

* **agent adapters:** task 1 - define unified types and AgentAdapter trait ([be89a01](https://github.com/airlock-hq/airlock/commit/be89a01cb9a810fabe421fbd0bd4bec03a0d9ee8))
* **agent adapters:** task 2 - implement StreamCollector utility ([7acee1d](https://github.com/airlock-hq/airlock/commit/7acee1db944b7f967847aea96148ed71ace384cd))
* **agent adapters:** task 3 - refactor Claude Code adapter to implement AgentAdapter ([df01693](https://github.com/airlock-hq/airlock/commit/df016931989ca620d7d331f953f1b95f5a49817f))
* **agent adapters:** task 4 - implement Codex adapter ([1a2f2e5](https://github.com/airlock-hq/airlock/commit/1a2f2e588d8d990d8a8d824c35e6a4d641f05663))
* **agent adapters:** task 5 - adapter registry, config, and CLI integration ([29c4197](https://github.com/airlock-hq/airlock/commit/29c41971247eb1bf1b793db1167ad8009a5d9a77))

## [0.1.1](https://github.com/airlock-hq/airlock/compare/airlock-v0.1.0...airlock-v0.1.1) (2026-02-19)


### Features

* **agent:** improve response handling and add structured output support ([7cffda0](https://github.com/airlock-hq/airlock/commit/7cffda08b0a46a65244e1cba3e49148aa1eba1ec))
* **approval-flow:** task 6 - Update pipeline resume for workflow/job/step model ([65d686a](https://github.com/airlock-hq/airlock/commit/65d686a3036d243696a899e0531468ecbf615272))
* **changes-tab:** add dark mode hook and refactor diff handling ([1e4c9fd](https://github.com/airlock-hq/airlock/commit/1e4c9fdc454a780e03fd6d441b2bb5b67cf7fb3d))
* **ci:** update CI workflow with new build steps ([8702cea](https://github.com/airlock-hq/airlock/commit/8702cead059b5fda08da5e185a2c76af828520a8))
* **cleanup:** task 11 - Clean up dead code and finalize ([f6b7579](https://github.com/airlock-hq/airlock/commit/f6b7579e45091efff5358c2acc9ad275ac71f338))
* **core:** add git push implementation ([3d7480d](https://github.com/airlock-hq/airlock/commit/3d7480db135b9c8dedb1dffc42814502c1abb6e0))
* **core:** add jj integration ([4f0ea82](https://github.com/airlock-hq/airlock/commit/4f0ea82e9c6bb575c927204311c1f889697574f5))
* **core:** add superseded flag to runs ([e2fed30](https://github.com/airlock-hq/airlock/commit/e2fed30154de578bf912d0b20adca6e502c63a8a))
* **core:** prioritize structured output in Claude responses ([d0d9dda](https://github.com/airlock-hq/airlock/commit/d0d9ddacfe343b352593a36f77bcc8c146c20294))
* **core:** use airlock config from latest push ([2e38415](https://github.com/airlock-hq/airlock/commit/2e38415a1d8924f9f4ef127b347b37b7159341fc))
* **daemon-hooks:** add loading handling and auto-refresh for health and repo hooks ([d795b9f](https://github.com/airlock-hq/airlock/commit/d795b9f396a01b0c40eead1196dac416e2b83554))
* design system update with landing page mock ([773b626](https://github.com/airlock-hq/airlock/commit/773b626d6c30a00370a983aa9fb5eae50d2ef28d))
* **design-system:** add status dot component ([011dbeb](https://github.com/airlock-hq/airlock/commit/011dbebbd67d3aa3ccb0eef6fc9f6f33cfd6d518))
* **design-system:** migrate UI components to shared design system ([1e149af](https://github.com/airlock-hq/airlock/commit/1e149afa9d2f78ed65f516f3f787efd3b8ca83b5))
* **e2e-tests:** task 10 - Update E2E tests for workflow/job/step model ([7a3e7d1](https://github.com/airlock-hq/airlock/commit/7a3e7d19250a2b33389fbc8fdf145cb03f8c7409))
* **eject:** add cleanup for inconsistent state ([3124426](https://github.com/airlock-hq/airlock/commit/312442626b67dce07e3a5510e702456aea5d0ded))
* **fixtures:** add new run detail fixtures and update repo list ([dfbe5b8](https://github.com/airlock-hq/airlock/commit/dfbe5b85b8af2957bf5ff8cdd512a269579a9caa))
* **fixtures:** add new run detail fixtures and update repo list ([aeccce0](https://github.com/airlock-hq/airlock/commit/aeccce0ae54746f7a2095c4ca0072ee4813388d6))
* **frontend-jobs-steps:** task 9 - Update frontend for jobs/steps hierarchy ([765b3b7](https://github.com/airlock-hq/airlock/commit/765b3b7f8804b540e94c9b84f28b3026cbc4f272))
* **git-hooks:** add banner to pre-receive hook ([60b1227](https://github.com/airlock-hq/airlock/commit/60b122796e321073ac6fa5b29f230cdcdaeaa403))
* homebrew installer ([61dee06](https://github.com/airlock-hq/airlock/commit/61dee066aa79fd51bf6474612a0164feb6539731))
* **init-eject:** task 8 - Update init and eject commands ([0cecfbe](https://github.com/airlock-hq/airlock/commit/0cecfbe4a7d0cd1398f146b2a303876c675ccfd7))
* **init:** add upstreamorigin remote and rewire origin ([002f931](https://github.com/airlock-hq/airlock/commit/002f9312e72bd028a8aae1577f12a45388ccda48))
* initial commit ([10f0b0a](https://github.com/airlock-hq/airlock/commit/10f0b0a30fbc7629d3b427bbce55335bd6580734))
* **pipeline-config:** task 1 - Rename "stage" → "step" and update types across airlock-core ([d28d6a1](https://github.com/airlock-hq/airlock/commit/d28d6a1341e2d34908cd08aa75d8df158d9eec50))
* **pipeline-config:** task 2 - Implement WorkflowConfig schema and multi-file loader ([d865874](https://github.com/airlock-hq/airlock/commit/d865874a1337e56e16875436946c616c96ed2aab))
* **pipeline-config:** task 3 - Update database schema (migration v8) ([c5ab80d](https://github.com/airlock-hq/airlock/commit/c5ab80dc2e63ea22a04fd189f42677eee0938675))
* **pipeline-config:** task 4 - Update IPC events and daemon types ([0391002](https://github.com/airlock-hq/airlock/commit/0391002f09afcc166b54dfa09cc9e29cdda66b17))
* **pipeline-execution:** task 5 - Rewrite pipeline execution for workflow/job/step model ([c1dc5d5](https://github.com/airlock-hq/airlock/commit/c1dc5d5720737942189214575a8bac39d547b5c5))
* **pipeline:** add default branch detection ([5aa16e1](https://github.com/airlock-hq/airlock/commit/5aa16e1e4d494013924fa5c69a60a1382a207fa5))
* **pipeline:** add lint stage ([45d119b](https://github.com/airlock-hq/airlock/commit/45d119bd911421b78911f7fcdaaccafb6f50c6d2))
* **pipeline:** rename actions to defaults ([3e54db9](https://github.com/airlock-hq/airlock/commit/3e54db9f4cb927b510bddcb186ee44bc0b74deea))
* **push:** handle deletion-only pushes and extract primary ref ([a313e08](https://github.com/airlock-hq/airlock/commit/a313e08e8ee66608bc989b271f7745fc8167765f))
* **repo-config-path:** task 1 - introduce REPO_CONFIG_PATH constant and update init flow ([69a3353](https://github.com/airlock-hq/airlock/commit/69a335392c6d0c77b7d09378d716fd303d931196))
* **repo-config-path:** task 10 - update spec and doc files ([1d896d3](https://github.com/airlock-hq/airlock/commit/1d896d3ad1fe78ba4ac9dbb5e6ca7c3514a63626))
* **repo-config-path:** task 11 - run full test suite and checks ([9583566](https://github.com/airlock-hq/airlock/commit/9583566b519ff6767d9eaa0eb6dd411770faf89f))
* **repo-config-path:** task 2 - update CLI user-facing output ([db62456](https://github.com/airlock-hq/airlock/commit/db624562d65b6a7088060fb962be9c14756103b1))
* **repo-config-path:** task 3 - update pipeline execution path ([b9f34c8](https://github.com/airlock-hq/airlock/commit/b9f34c83f648804d2e1ff842f019e10a7bcd172c))
* **repo-config-path:** task 4 - update daemon config handlers ([b07e467](https://github.com/airlock-hq/airlock/commit/b07e4671574695cb3ef0a23f6f96c9ea11d25bb5))
* **repo-config-path:** task 5 - update IPC doc comment ([d8ced0d](https://github.com/airlock-hq/airlock/commit/d8ced0de3c69975e8250f2de1e05925a0fe756d6))
* **repo-config-path:** task 6 - rename global config from config.yaml to config.yml ([31086f2](https://github.com/airlock-hq/airlock/commit/31086f2da4de0df489f69f610389603a2d320924))
* **repo-config-path:** task 7 - update frontend references ([5fd0b67](https://github.com/airlock-hq/airlock/commit/5fd0b6740cbe7ec8745ca8ee7a5248fd3ac81e3b))
* **repo-config-path:** task 8 - update JSON fixture files ([f47d424](https://github.com/airlock-hq/airlock/commit/f47d4240a2fd641acf5c3141b941a787c77d17bf))
* **repo-config-path:** task 9 - update Rust tests ([faeba06](https://github.com/airlock-hq/airlock/commit/faeba0607933ccf23b5208d6aa10563db6fb51e5))
* **run-detail:** add URL tab and stage persistence ([2510514](https://github.com/airlock-hq/airlock/commit/251051426d510567838136f6283462f12b0c8ab8))
* **runs:** improve runs UI and telemetry ([7f033e2](https://github.com/airlock-hq/airlock/commit/7f033e2ab137d45b5323aac5338890767c944041))
* sort artifacts by time ([11168b6](https://github.com/airlock-hq/airlock/commit/11168b6b91565273c76aa0f5f0c98e1502036239))
* **stage-loader:** task 7 - Update stage loader for new terminology ([5607acf](https://github.com/airlock-hq/airlock/commit/5607acf7de664e3325bde4a6716fcea418106be5))
* **stages:** add create-pr and push stages ([a89dcb3](https://github.com/airlock-hq/airlock/commit/a89dcb399e14fcf320020470ea63c70349882b51))
* **stages:** improve description and test stages ([b00b5dc](https://github.com/airlock-hq/airlock/commit/b00b5dc270c7e65df5ad060b392a474432a01b91))
* **stages:** write stage scripts inline ([3731eb2](https://github.com/airlock-hq/airlock/commit/3731eb221ad24ae3c18093628fb33c9d5604f6d8))
* **stages:** write stage scripts inline ([ea31c66](https://github.com/airlock-hq/airlock/commit/ea31c66ecdd1a823c2343caadf1fd8b5539a6128))
* **storybook:** add design-system story files for app UX ([4813fd7](https://github.com/airlock-hq/airlock/commit/4813fd746bf7274bc5bb9ee5b8e75d2d62264fa4))
* tauri app icons ([d7b9a6e](https://github.com/airlock-hq/airlock/commit/d7b9a6ed4b888f71161da7a28bf4aa8e877786f2))
* upgrade to tailwind v4 ([e08e241](https://github.com/airlock-hq/airlock/commit/e08e24188995cdf1a8bf91e0dfd353beec55970f))


### Bug Fixes

* add created_at timestamp to artifacts ([31a18a1](https://github.com/airlock-hq/airlock/commit/31a18a12865fb01955ed0bc638f558a0f46fd70f))
* add missing multi-job demo fixtures ([461f41e](https://github.com/airlock-hq/airlock/commit/461f41e79306bc531876fb1801f3500741352c3d))
* add repository metadata to design system package ([e259b34](https://github.com/airlock-hq/airlock/commit/e259b347eb2fea8cecd1a7ed5887843fb8d1f999))
* app ui revamp aligns with design system ([f609131](https://github.com/airlock-hq/airlock/commit/f609131ac7443277ebb3bcb52710416689ebd687))
* CI check failure ([9da770d](https://github.com/airlock-hq/airlock/commit/9da770d6b9f862d7fdaa0420c52ff00a0add94b8))
* **core:** ensure tracking after eject ([742fc23](https://github.com/airlock-hq/airlock/commit/742fc23b124da5f9f0b9f9cd88f50ae0f05fcfee))
* **git/hooks:** update post-receive message wording ([64488cb](https://github.com/airlock-hq/airlock/commit/64488cb02299735c4a9d579a0cc80b93bc44d6a5))
* improve agent error handling and logging ([855dbc0](https://github.com/airlock-hq/airlock/commit/855dbc0ba6f30c499d02cf440755f106906f3179))
* improve dev server cleanup in Makefile ([4712040](https://github.com/airlock-hq/airlock/commit/47120400ad03f4fccb44db36f90c9edc93b159f3))
* **layout:** adjust navigation spacing and font vars ([e86fa09](https://github.com/airlock-hq/airlock/commit/e86fa09057248f970e4d4f790d9f8d750ae54c6c))
* more robust ssh key handling ([56c4c0f](https://github.com/airlock-hq/airlock/commit/56c4c0f60e2a41585bf1af7f39439d2f1e5916ee))
* move atmosphere components to design system ([a91abce](https://github.com/airlock-hq/airlock/commit/a91abce96e27c8bf12d665593f3d017cc12da4fd))
* npm publish ([#4](https://github.com/airlock-hq/airlock/issues/4)) ([d98d27d](https://github.com/airlock-hq/airlock/commit/d98d27db04cb957771ed0f0dcda52ace829d37ec))
* open external URL in browser ([7feb2bb](https://github.com/airlock-hq/airlock/commit/7feb2bb65ed3296641ec60f5abf5f2fb9a9b894f))
* **pipeline:** force push handling ([07f0869](https://github.com/airlock-hq/airlock/commit/07f08693ef15323abc43965b2648faf3bb834db8))
* prevent failed runs from restarting ([21df00d](https://github.com/airlock-hq/airlock/commit/21df00d89efbfe106b201c54d4c67d12f5e2a5d1))
* tauri ipc bridge for new data model ([16b82e1](https://github.com/airlock-hq/airlock/commit/16b82e11a9ad228dcdf8714ffa077de1fe3544e0))
* **util:** add detailed artifact loading and subdirectory handling ([9f8869f](https://github.com/airlock-hq/airlock/commit/9f8869fd92bf27159d938c6dfea455488c4c25d8))
