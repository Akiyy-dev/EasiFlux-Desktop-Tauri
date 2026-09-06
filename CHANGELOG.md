# Changelog

## [0.5.0](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/compare/easiflux-desktop-tauri-v0.4.1...easiflux-desktop-tauri-v0.5.0) (2026-09-06)


### Features

* **notifications:** add account-aware notification store ([6d536f1](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/6d536f1378d11482afbe4c52fdb20843198cc706))
* **notifications:** add authoritative notification service ([471d639](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/471d63958aa2b71e985454800690326b657937be))
* **notifications:** add frontend notification adapters ([ce60531](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/ce60531cb429ced94a90390b5f2af907bf275a2e))
* **notifications:** add notification center popover ([13332f6](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/13332f65e8e19471787e460ba337ffba5783cc73))
* **notifications:** add notification preferences and cleanup ([d8ca822](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/d8ca822ddea16b9369a2f2ded27a889b2f21955b))
* **notifications:** add persistent notification center ([b628874](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/b628874ca5d0577f336de3be22b07556c9ec2dc0))
* **notifications:** bridge account failures and cleanup ([f47eec9](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/f47eec9b31dd69d88f85ac7433fcca6ee7394124))
* **notifications:** define domain and toast settings ([5577422](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/5577422f48c22c166b33eee7366a732e9a01a177))
* **notifications:** expose notification IPC and maintenance ([cc40194](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/cc401948022db6690ae7d3a1aeec41f3239c7ebd))
* **notifications:** observe connection and environment incidents ([af53fc4](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/af53fc45db699c1c697ad16b810250fb15c70b6f))
* **notifications:** observe order and risk outcomes ([baf76b0](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/baf76b0c18999b7c6d4d9dbb8b85938e68f83c09))
* **notifications:** observe order and risk outcomes ([12e2cf5](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/12e2cf518b3ea520fb5e21160d14837379a9074f))
* **notifications:** persist atomic notification snapshots ([2fd2968](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/2fd2968aaebb6626d0c1196552333ad8ec6eed26))
* **scheduler:** support dynamic market fallback interval ([6efa856](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/6efa8560d54354e1802182a4079f6746cf0a1073))
* **settings:** add account settings page ([2ed3f45](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/2ed3f450db95535a9aee30f7b5d7a74b6d457942))
* **settings:** add atomic general settings update ([e06a2d6](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/e06a2d6dd5f4d822e7741939fdec2ed113dded7d))
* **settings:** add full-page settings center ([96b653d](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/96b653de43e518aa84d835b903141379e5a0eecb))
* **settings:** add full-page settings navigation ([98dece7](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/98dece78f1e482d6eaee44b9fb4638f5e71ac218))
* **settings:** add general settings autosave ([c658a95](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/c658a9537c98ef4264bface2940c1328884c4551))
* **settings:** add general settings panel ([475733f](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/475733f226b227d6377bdb6cc67de393e04dd1b3))
* **settings:** add settings center presentation ([2bef598](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/2bef598955c9f098cefb27b9c5b67591769a0bfc))
* **settings:** separate credential quick setup ([46a4a47](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/46a4a473777f185722be7c233c32fce85b78e3bc))


### Bug Fixes

* **connection:** bind completion to caller lifetime ([d0aceda](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/d0aceda87cd85b86191f6b4c4130babffd101545))
* **connection:** ignore stale connect completions ([9225bf6](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/9225bf6b191979247734302b4462c8ec674d7d90))
* **connection:** make reconnect lifecycle reentrant-safe ([5e5b9cb](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/5e5b9cbb60ef2a24b1b03218419bd336b16292d6))
* **connection:** preserve concurrent reconnect intents ([801ce1a](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/801ce1aa9edd0353acad8c3e4abe1448fc4b68b4))
* **connection:** preserve latest reconnect intent ([2e83620](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/2e836209f1a59300f85b94e2e29a983dfbc0afd2))
* **connection:** preserve operation ordering ([2c1f126](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/2c1f126f0319774ad57552c054d869c0bc8d59be))
* **market:** commit scheduler kline visibility coherently ([422fd92](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/422fd92c02cecafc6f5115c65aa0aa5608096928))
* **notifications:** align IPC and scheduler lifecycle ([f958f97](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/f958f97995c023681f774673af789bfd6ef1c4f3))
* **notifications:** cancel stale settings navigation ([b207b67](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/b207b6741779d097184068025510ede6944a52eb))
* **notifications:** close delivery ownership gaps ([ac590ee](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/ac590ee9048b01e5820abc8e655aab396dc9d0e2))
* **notifications:** close final integration gaps ([fdc0727](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/fdc0727dbd8965363fe0b2a933af6b5b1c5f3e35))
* **notifications:** close persistence edge cases ([cb9020e](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/cb9020e504d97413810a10dd12cadf1cbfb2f1c8))
* **notifications:** close producer ownership gaps ([16d1fda](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/16d1fda23aca5c70ae833998220cdc3d3fda3db9))
* **notifications:** constrain semantic toast params ([ed915d4](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/ed915d4086c6a0cf29def0c49221ba572d06189b))
* **notifications:** enforce single delivery ownership ([ed12dc6](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/ed12dc65faf5a3de80a95007e130180ed505bebe))
* **notifications:** harden notification content validation ([3156628](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/315662830aedb52c5de2c341dd769a51837692af))
* **notifications:** harden order outcome retries ([4c88df7](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/4c88df7c609b1fa0d6ba20c5c8290be818d0d7a3))
* **notifications:** harden popover interaction ([4d1ed6e](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/4d1ed6e3c613d2c432c5a25baeda6b6f02f0b634))
* **notifications:** harden scheduler generation teardown ([56f63c4](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/56f63c4a9eb964c47ebec119b649e864947cad00))
* **notifications:** harden service lifecycle invariants ([54e49e1](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/54e49e10a35dbf26aea2a46cb7e678524b18fc99))
* **notifications:** harden store recovery guards ([0605107](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/0605107fea2f2074849bdb49d5f2f3c5008809a2))
* **notifications:** make scheduler cancellation safe ([cc37be0](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/cc37be094e924898a72e3c8f8c5769676636f70b))
* **notifications:** preserve dirty refresh feedback ([4e4e27b](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/4e4e27bdb46c2e9e3e20ec382762dea7712ef068))
* **notifications:** preserve error delivery ownership ([cfbe43c](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/cfbe43c5a52a9e9bcaf3a1176d3dcdf82be77a1e))
* **notifications:** preserve focus through popover transitions ([fc6cf18](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/fc6cf188623658d13af882a667b1b3d2629c0329))
* **notifications:** preserve session replay markers ([e76b857](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/e76b85799b1caa90585c2a77901450f413b04853))
* **notifications:** preserve store response authority ([c02d672](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/c02d6721f2157e19b1708b2bd217bdb2c5b3db96))
* **notifications:** prevent session redelivery ([87e3430](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/87e3430335d8451d8deb840c28646f1013344561))
* **notifications:** rearm only after authenticated recovery ([61c1bcf](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/61c1bcff172de8f80e09db393619ae66ba9bafca))
* **notifications:** reconcile canceled event refreshes ([2919ebe](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/2919ebe709b8bb1855b588daeee1e55e0b57b5c9))
* **notifications:** retain failed refresh recovery ([d084449](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/d084449107664fe32411dc0a9590a857ee4f2ba7))
* **notifications:** validate client replay targets ([d7a1fcf](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/d7a1fcfdd4c765bc0d13534f11a6adbdc6250f8c))
* **scheduler:** harden notification lifecycle recovery ([9bd16ad](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/9bd16add1712951e6fc104b1b10ce03392ad4095))
* **scheduler:** linearize lifecycle ownership ([acb6b50](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/acb6b50f29fa80fe9bc3a5058658660bdaedb62f))
* **settings:** apply settings child presentation styles ([4d03207](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/4d03207203fbe4275251644e674c486277b66a32))
* **settings:** await reconciliation during autosave disposal ([2a89f49](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/2a89f49a2274f22c47d85608a4226e70d271ad64))
* **settings:** bind account reconnect attempts ([6dd5dfa](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/6dd5dfad550493b4415feb40f3cd43157dc26a9c))
* **settings:** cancel stale quick setup side effects ([3c312ba](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/3c312ba623c1a713ea5b23a4326c4455264ae195))
* **settings:** preserve committed reconnect intent ([6e38626](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/6e386266bf997f53c1bdb4057aaaacdf655fca59))
* **settings:** preserve save error across reconcile throws ([a69d2d1](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/a69d2d1f746d806fc922afc83193f65d11179dc4))
* **trading:** correct risk checks and prevent duplicate orders ([0caa17a](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/0caa17a47293d38ff5e3ad5e57772373e31757ee))
* **trading:** correct risk checks and prevent duplicate orders ([44c3974](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/44c39748357140ac0b054c25c925e5cbbe292947))
* **trading:** preserve committed auth notification ([652ffc6](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/652ffc6bf26a380f5601a41cf1b82e6ceeb9676b))

## [0.4.1](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/compare/easiflux-desktop-tauri-v0.4.0...easiflux-desktop-tauri-v0.4.1) (2026-08-15)


### Bug Fixes

* **dev:** prevent Vite from watching Cargo target artifacts ([ab7a6e3](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/ab7a6e3b14968dd8a513cd1ab8f6a57823f14b83))
* ignore Cargo target artifacts in Vite watcher ([7fc158d](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/7fc158d3f1a0b7bed71d3557ca6e7ee3c6a92313))

## [0.4.0](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/compare/easiflux-desktop-tauri-v0.3.0...easiflux-desktop-tauri-v0.4.0) (2026-07-30)


### Features

* **account:** add account profile lifecycle ([5ca48e8](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/5ca48e8285489326213001efd3d5a2296c67de54))
* **account:** add read-only asset overview ([f87fc64](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/f87fc64cbbba8e637746e11cadc73c3887c0358e))
* **architecture:** unify data sync and realtime recovery ([93a850e](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/93a850e00b3ed7224f849b035c424cae3c3752aa))
* **architecture:** unify data sync and realtime recovery ([adca1a2](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/adca1a2d5462b94fa89e830407c68938b826e020)), closes [#9](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/issues/9)
* **chart:** add KLineCharts workspace ([953f633](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/953f633bd058ff87b71cc593843567b44c0598f0))
* **chart:** add KLineCharts workspace ([482e0ce](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/482e0ceb8c54147b7f0bba56eb7eb2bc8fec3eb2))
* **news:** add news center ([13f8876](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/13f8876bb19d164fa3c5fe2b9a24e4143d797110))
* **news:** add News Center ([8e247cc](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/8e247cc2e42ab5f6f5f043dd5c1309611eab6443))
* **news:** embed release credentials ([d9e0253](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/d9e0253f18857eeee84de8703ea127317063c4be))
* **risk:** expose configurable daily risk status ([4a05b72](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/4a05b7258e04ba0e4dc5d025fe028ea4ca811174))
* **time:** implement global time service and unified scheduler ([9c39345](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/9c393459460cc7bc5ed97bc04c723726dbf479f9))
* **ui:** add app shell navigation layout ([17b6276](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/17b6276678817f21352cf1e2ec6ab38feeb30c11))
* **ui:** add dashboard home page ([713d417](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/713d4177eb3bf671a615c428c18f3e02279b380e))
* **ui:** add TanStack order center tables ([3d1b68b](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/3d1b68bc1c334c53a4ab463a4b46b72adf81d489))
* **ui:** enhance dashboard functionality and user experience ([0600a1c](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/0600a1c545eb0768895a08a9f7ee53d1449d2f20))
* **ui:** establish design system foundation ([6cb15e7](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/6cb15e7db8a41b458ed5f2f87d7d40f1ec22d66b))
* **ui:** formalize design system tokens and components ([a8a7ec8](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/a8a7ec84220aa3f69f2983dc244ce19d8da67d16))
* **ui:** integrate account and risk center navigation ([700a0a5](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/700a0a572d1da20778ecb2f86ba888586823b2dc))
* **ui:** redesign trading page layout ([6fbf9ee](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/6fbf9eeaa4269c82adc150461f9b2fdb9f502b46))
* **ui:** scaffold React design system foundation ([f4423f4](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/f4423f4d2fd2fac2f9358fd25431f7a615d7dcf1))
* **ui:** upgrade kline chart and trading panel ([64a95d8](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/64a95d882c200c3a78e7691187d96d6cfd634b08))
* **ui:** Vue trading UI refactor with design system ([6b6a81d](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/6b6a81dcb725ef7b0d311ec5e51fe85c085f34ee))


### Bug Fixes

* **app:** harden risk controls and refresh flows ([fff993c](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/fff993c2ce3600cfa11b049b32107a955e6be4d5))
* **app:** harden risk controls and refresh flows ([5aa496b](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/5aa496b3427b360aff80e94183991f37deee4dab))
* **chart:** allow closing after workspace flush ([18b2f96](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/18b2f96a267eeb3204d08abc5b37abf5e3714aa6))
* **chart:** restore navigation and chart mounting ([3a858b0](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/3a858b04e6c0b6a9f9e5612367781763a14c67c7))
* **connection:** unblock account initialization ([82fcdf9](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/82fcdf9d6f39e83cad4d7f7b0321a2aaff9075c6))
* **deps:** update vulnerable transitive dependencies ([194d521](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/194d5211ab2ad807bb78b22ac3cc3c65e1aaa6ed))
* **i18n:** localize account and risk center copy ([24353f9](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/24353f984887ace987c4afc3e29da44516436282))

## [0.3.0](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/compare/easiflux-desktop-tauri-v0.2.0...easiflux-desktop-tauri-v0.3.0) (2026-07-05)


### Features

* **api:** align REST and WebSocket with EasiFlux-SDK v0.3 ([25fbe6d](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/25fbe6d5f90eb3c8e4b46f8f470a298e052bc045))


### Bug Fixes

* **auth:** align SDK signing and fix market panel display ([8880a16](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/8880a16ed83882343582a69cca6b946e14bdde29))
* **ci:** satisfy void return type in refreshPostConnectData task ([928a20b](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/928a20bdefe5d2e7f8750ef02d32c8f1ccbab64d))
* **connection:** align save-and-connect with test credential path ([0d5f569](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/0d5f56960a7dd0fcc13df9fb635eedd7c3527b88))
* **connection:** decouple API connect from WebSocket startup failures ([d2ab829](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/d2ab829bdb449ce1088653892bad2f99b7c0e167))
* **market:** credentials, 24h pct semantics, and kline persistence ([b0c1ebe](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/b0c1ebe4108de800e3427245d5434688c656f7e0))
* **market:** harden polling and incremental K-line refresh ([8c7efd9](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/8c7efd9cf564de0dfe8155b4547087d0d520abf7))
* **market:** merge WS ticker deltas and restore private panel data ([1a56c92](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/1a56c9246dbc5dc714d653ed4bc90c9de23cf814))
* **market:** restore post-login data and migrate K-line chart ([662d94a](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/662d94a65e81d7f5a13aacc04e5376c8adb2f5d7))
* **market:** restore post-login ticker, account, and kline data ([0dcc8c5](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/0dcc8c51f042162a31437e58fb426792ad522135))
* **trading:** default coin=USDT for private queries and harden klines ([802abc1](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/802abc187aad6af45683af93a839a5d37fb9aa3d))
* **trading:** restore futures panels, private data, and analytics ([a6acbe9](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/a6acbe98b2368289fd4430d7c97953ebad1d9a43))
* **trading:** restore private panels, analytics, and kline reset ([999cd19](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/999cd198dc02085d17abb5473ecd7b20a3edddd5))
* **trading:** unify private panel refresh and restore table display ([3bdc30a](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/3bdc30acc2ced0e096328e77d0af1226964e4c4c))
* **ws:** align WebSocket flow with SDK and split connection status ([82033b0](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/82033b037166f55c692582d459847af5d50faef1))
* **ws:** SDK v0.3 alignment and connection reliability ([1b16646](https://github.com/Akiyy-dev/EasiFlux-Desktop-Tauri/commit/1b16646af316387e1887b452ed9b9845b91f560e))
