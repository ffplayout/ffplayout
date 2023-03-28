# Changelog

## [0.17.0](https://github.com/ffplayout/ffplayout/compare/v0.16.7...v0.17.0) (2023-03-28)

### ffpapi

- use extensions from config and extra_extension from frontend [e363077](https://github.com/ffplayout/ffplayout/commit/e363077d30c47bb42adf39728f0f961cf1cee903)
- support folder list for playlist generation [e752a7a](https://github.com/ffplayout/ffplayout/commit/e752a7a95110b35d29538b8b2221e3f79c065b31)
- add piggyback mode [7e5a391](https://github.com/ffplayout/ffplayout/commit/7e5a391e3d77f67b243026d3c4c1fded583cd2d9) [6c5264e](https://github.com/ffplayout/ffplayout/commit/6c5264ea5fe123b0718a4525605761ee1971ffae)

### ffplayout

- fix v_in in custom filter [537f664](https://github.com/ffplayout/ffplayout/commit/537f664c067a122e31c06c196d354dc4bfd7fed3)
- add audio only mode [537f664](https://github.com/ffplayout/ffplayout/commit/537f664c067a122e31c06c196d354dc4bfd7fed3)
- get correct error level from config [c0740fc](https://github.com/ffplayout/ffplayout/commit/c0740fc8303f08cb57fe956de805290a705e5a28)
- fix logo path on windows system #291 [3328aaa](https://github.com/ffplayout/ffplayout/commit/3328aaac6a6b814ce491ff3c07e580136ea453dd)

### frontend

- rewrite frontend to nuxtjs 3

### Development

- update ffmpeg action [b2093dd](https://github.com/ffplayout/ffplayout/commit/b2093ddf352964115c11bc09c2849c8491ee1156)
- set version and other metadata globally [3b61d09](https://github.com/ffplayout/ffplayout/commit/3b61d09809db4c6e1c02c5b8f0bb22eab9f4568d)
- another filter test [0ed6add](https://github.com/ffplayout/ffplayout/commit/0ed6add25fe43b6137400fbbda68b641c766f734)
- fix "error: unpacking of archive failed: cpio: Bad magic" [8a2e1e7](https://github.com/ffplayout/ffplayout/commit/8a2e1e7d3dccf76a078a16e3845bd0b4398d2f3f)

### Documentation

- add infos about ingest errors [c57d497](https://github.com/ffplayout/ffplayout/commit/c57d497dee9afb47a9a388d4c72feeafd55f8867)
- update install instruction [d9952c8](https://github.com/ffplayout/ffplayout/commit/d9952c88fc5a50fd797161745ad0e95ec79099ef)
- add docker documentation [505ae23](https://github.com/ffplayout/ffplayout/commit/505ae23a1c2d69837ec1075be33d10f91eb6363f)

## [0.16.7](https://github.com/ffplayout/ffplayout/compare/v0.16.6...v0.16.7) (2022-12-20)

### ffplayout

- log error only when fdk_aac is in use [8ac3688](https://github.com/ffplayout/ffplayout/pull/249/commits/8ac3688d2bd178db9b5a54efa1bde4e688432564)
- make libx264 optional [1491f46](https://github.com/ffplayout/ffplayout/pull/249/commits/1491f46e3dbb9a4dfa14fe2ab3680c6d0cc89b3d)
- catch empty program list in [#101](https://github.com/ffplayout/ffplayout-frontend/issues/101) [850a48e](https://github.com/ffplayout/ffplayout/pull/249/commits/850a48ed43a671e7a0d924510b80592e489fff94)
- update packages, set correct port [a3ce014](https://github.com/ffplayout/ffplayout/pull/249/commits/a3ce014444672704d9c33af6f6105f57c40a544d)

### frontend

- remove dotenv, update packages [3ddec8c](https://github.com/ffplayout/ffplayout/pull/249/commits/3ddec8cf19db692412c603718038cf3f0ffa7815), should fix [#101](https://github.com/ffplayout/ffplayout-frontend/issues/101)
- suppress 408 error [b812de9](https://github.com/ffplayout/ffplayout/pull/249/commits/b812de97470fd21e8734fe6e5282cc0e871384ca)

## [0.16.6](https://github.com/ffplayout/ffplayout/compare/v0.16.5...v0.16.6) (2022-12-17)

### ffplayout

- add logo scale
- add optional ingest_level
- set windows title in desktop mode [f388820](https://github.com/ffplayout/ffplayout/commit/f38882032f809f094cef895beff07582f0fe9b8f)

### Development

- migrate to Rust 1.66.0 [f388820](https://github.com/ffplayout/ffplayout/commit/f38882032f809f094cef895beff07582f0fe9b8f)
- update packages [f388820](https://github.com/ffplayout/ffplayout/commit/f38882032f809f094cef895beff07582f0fe9b8f)

## [0.16.5](https://github.com/ffplayout/ffplayout/compare/v0.16.4...v0.16.5) (2022-11-28)

### ffpapi

- init db needs its own connection, fix #241 [edfff82](https://github.com/ffplayout/ffplayout/commit/edfff8269b660ef149023d859451b94c198474ba)

### ffplayout

- change StartLimitIntervalSec in systemd service [010fc29](https://github.com/ffplayout/ffplayout/commit/010fc29b38129e503c12b80a6710dccd90056851)
- get list of filters and libs for future usage (#201 #219) [52856d3](https://github.com/ffplayout/ffplayout/commit/52856d3f0945cae310b7bbae39ae0a5626b4822f)

## [0.16.4](https://github.com/ffplayout/ffplayout/compare/v0.16.3...v0.16.4) (2022-11-21)

### ffpapi

- add enpoint for gettting program infos, mainly usefull for generating xmltv [f576ded](https://github.com/ffplayout/ffplayout/commit/f576dedcb9ebac259ec2283a622cd521a2f614b8); [aa820b2](https://github.com/ffplayout/ffplayout/commit/aa820b29c2be0b2b6c946466ca6a274e8771ce4d); [0d87bae](https://github.com/ffplayout/ffplayout/commit/0d87baece7df9dfb40969a5b34c5cf944967aba0)
- use only one DB pool and share them with web::Data [a5f0813](https://github.com/ffplayout/ffplayout/commit/a5f0813d2acd1a41b132d114fa4dfa1ea6150c45); [4122aaa](https://github.com/ffplayout/ffplayout/commit/4122aaa7a6b52038dc61f643deeb30fdeee7e09e); [5780de3](https://github.com/ffplayout/ffplayout/commit/5780de38c40429402ead35d2815aa3d99feaa3be)

### ffplayout

- limit restart count from systemd service [694c9f8](https://github.com/ffplayout/ffplayout/commit/694c9f8c4b75f5e1d3c219dfe77317f1d2788627)
- update dependencies, migrate chrono to 0.4.23 [8be1992](https://github.com/ffplayout/ffplayout/commit/8be199222e82c69fa7bcf23c87511642aec7a156)

### frontend
- update dependencies

### Dokumentation

- fix api examples [#232](https://github.com/ffplayout/ffplayout/discussions/232); [#238](https://github.com/ffplayout/ffplayout/issues/238); [694c9f8](https://github.com/ffplayout/ffplayout/commit/694c9f8c4b75f5e1d3c219dfe77317f1d2788627); [8f84b70](https://github.com/ffplayout/ffplayout/commit/8f84b702057b4bcbd6103fe8f8d468f36c09ffa5)
- set EBU R128 loudness normalization again to experimental [f576ded](https://github.com/ffplayout/ffplayout/commit/f576dedcb9ebac259ec2283a622cd521a2f614b8)
    - the audio quality is not very good and it is not recommended to use the filter if a good quality is desired
    - maybe this function will be removed again in the future

## [0.16.3](https://github.com/ffplayout/ffplayout/compare/v0.16.2...v0.16.3) (2022-11-04)

### ffplayout

- escape characters in drawtext filter [76e26f0](https://github.com/ffplayout/ffplayout/pull/223/commits/76e26f0f704948371638308cb844ee560d679e62)
- revert to old audio codec settings [0e3b9e3](https://github.com/ffplayout/ffplayout/pull/223/commits/0e3b9e3f806f06177883226ebbe49097292df0c7)
    - Some how with s302m there is a smaller time delta. MP2 works in general, and also better with loudnorm filter, but s302m is uncompressed and time stays more in sync.
- expose audio channel layout to the config [#222](https://github.com/ffplayout/ffplayout/issues/222), [960280f](https://github.com/ffplayout/ffplayout/pull/223/commits/960280f1423d159fb8a4af79a14f97b35840f3a9)
- ignore muxed as a private data stream warning, validate channel count [6149288](https://github.com/ffplayout/ffplayout/pull/223/commits/6149288d2fbeef8d122c9e44b7420dc795f67d5b)

### Development

- fix cross compile for osx [5cbf5e7](https://github.com/ffplayout/ffplayout/pull/223/commits/5cbf5e7a4c20d9560dada978bad51a7556031b73)

## [0.16.2](https://github.com/ffplayout/ffplayout/compare/v0.16.1...v0.16.2) (2022-10-26)

### ffplayout

- ignore more ffmpeg errors and ignore them also on ingest server [2f8c2de](https://github.com/ffplayout/ffplayout/pull/221/commits/2f8c2deebc857c23f0bdc96ef977aaa174981fd3)
- update dependencies [bdf43f7](https://github.com/ffplayout/ffplayout/pull/221/commits/bdf43f7e6bd765ebb88afac7761a0a246b5cdfb4)
- fix null output, when is set per command line parameter [5b910d6](https://github.com/ffplayout/ffplayout/pull/221/commits/5b910d6e65d6cd1800fffe914a859a2b121be3cf)
- revert to video bitrate and mp2 audio codec [c326c3b](https://github.com/ffplayout/ffplayout/pull/221/commits/c326c3b61fdedf2cd4f609c74160ad5e3c470f43)
    - When video bitrate is not fixed the delta delay is more unstable and can reach error threshold. Same is with audio codec pcm_bluray, maybe because it changes the format to m2ts. s302m would be best option, but is not working correctly with loudnorm filter.
- print version in debug level [241d8ee](https://github.com/ffplayout/ffplayout/pull/221/commits/241d8ee3f661f0c2585cd288a695cb5099b05677)

### Dokumentation

- add info for srt ingest [2f8c2de](https://github.com/ffplayout/ffplayout/pull/221/commits/2f8c2deebc857c23f0bdc96ef977aaa174981fd3)

## [0.16.1](https://github.com/ffplayout/ffplayout/compare/v0.16.0...v0.16.1) (2022-10-25)

### ffplayout

- rearrange custom filters (fix missing output mapping on multiple outputs) [9cb3a62](https://github.com/ffplayout/ffplayout/pull/217/commits/9cb3a6206938adcf1fbe4ce0ec763cad9e812c76)
- switch decoder audio codec to pcm_bluray [8b3a80f](https://github.com/ffplayout/ffplayout/pull/218/commits/8b3a80f5602eda240c6a59178c33886c9e81cb1d)
- deserialize drawtext message with struct object and add single quotes around values [1373182](https://github.com/ffplayout/ffplayout/pull/218/commits/1373182c2ad457d34bff449385e73203b9ba5791)
- update dependencies [a246a60](https://github.com/ffplayout/ffplayout/commit/a246a6018eb024cbeac11dd206b76eaffd7fd20c)

### Development

- fix deb and rpm bundle [79e4d5d](https://github.com/ffplayout/ffplayout/pull/217/commits/79e4d5dda05e715df96a38070466ea7a4c8378b2)
- add subtitle example [d0ef717](https://github.com/ffplayout/ffplayout/pull/217/commits/d0ef71767b2af7d6053aeb83e3a6906fb84c984c), [e72967a](https://github.com/ffplayout/ffplayout/pull/217/commits/e72967a21c14ee8c71e18085dc397740d3586d01)
- use NODE_OPTIONS for nodejs 18 [bcf212d](https://github.com/ffplayout/ffplayout/pull/218/commits/bcf212d8de6c2b87571e73cd73023af0e4b7941b)

## [0.16.0](https://github.com/ffplayout/ffplayout/compare/v0.15.2...v0.16.0) (2022-10-19)

### ffplayout

- add option to convert text/m3u file to playlist,fix [#195](https://github.com/ffplayout/ffplayout/issues/195), [69a3e59](https://github.com/ffplayout/ffplayout/commit/69a3e59e3548f082f68ef176acd7043ee0f06902)
- ignore some harmless ffmpeg errors [2ebb4c6](https://github.com/ffplayout/ffplayout/commit/2ebb4c6822e5721beedb3988fbe915c229ee2f20)
- only seek in when seek value is over 0.5 [9d094d9](https://github.com/ffplayout/ffplayout/commit/9d094d983878563960fb7fc222ce9877a583e4e9)
- use realtime video filter only [9d094d9](https://github.com/ffplayout/ffplayout/commit/9d094d983878563960fb7fc222ce9877a583e4e9)
- update dependencies
- add at least anull filter [dcc4616](https://github.com/ffplayout/ffplayout/commit/dcc461642169bf2c5db812c2a806e6d64baf8101)
- multi audio track support, fix [#158](https://github.com/ffplayout/ffplayout/issues/158) [#198](https://github.com/ffplayout/ffplayout/issues/198), [c85e550](https://github.com/ffplayout/ffplayout/commit/c85e5503b432f1c44fcbf11870d2dfc140c65db9)
- add filter type enum [1d11d36](https://github.com/ffplayout/ffplayout/commit/1d11d36ef9cccbdfe215adfe970e8c4219774227)
- switch most integers to i32 [c3b5762](https://github.com/ffplayout/ffplayout/commit/c3b57622bbc19e55d203b5ee66b76ac3307fef10)
- fix wrong log message in HLS mode: Decoder -> Encoder [8a5889b](https://github.com/ffplayout/ffplayout/commit/8a5889be3710e92d88c4ad4815cf5805a77f84c9)
- wait for ffmpeg in validation process to be closed, fixed system zombies [8fe7b87](https://github.com/ffplayout/ffplayout/commit/8fe7b87644b5216b3a39b21264d2246ec610ee10)
- add tests, mostly input and output parameter tests [87c508b](https://github.com/ffplayout/ffplayout/commit/87c508be541cacbbae5d9efedfb903506e573ad5)
- add test files [87c508b](https://github.com/ffplayout/ffplayout/commit/87c508be541cacbbae5d9efedfb903506e573ad5)
- add ProcessMode enum [61f57e2](https://github.com/ffplayout/ffplayout/commit/61f57e2f9e0498d2939f57fade0daf2efbdc2824)
- multi audio outputs [06b5d6a](https://github.com/ffplayout/ffplayout/commit/06b5d6a2275f286f165d173b834f92e18e0514ac)
- fix case when video has no audio, but separate audio is set [a93440e](https://github.com/ffplayout/ffplayout/commit/a93440e06b4533689beae4dd6b07767db300757a)
- allow loudnorm on ingest only [69b6207](https://github.com/ffplayout/ffplayout/commit/69b62071656c3d4a3ab8b0f84341c1f584d47e40)
- use named drawtext filter instead of getting its index [84addbc](https://github.com/ffplayout/ffplayout/commit/84addbcb2a21725f2de34d2b4602ee95f1753311)
- use filters struct for stream encoder [096c018f](https://github.com/ffplayout/ffplayout/commit/096c018fe38a0653c1dfc279775b7131584f5463)
- unify null output [31b72db](https://github.com/ffplayout/ffplayout/commit/31b72db10640a6508ab50eca43625f04c26f2030)
- build output filters from scratch, fix [#210](https://github.com/ffplayout/ffplayout/issues/210), [09dace9](https://github.com/ffplayout/ffplayout/commit/09dace92f4100aecfc92ad7df06f1e8b7174f690)
- simplify prepare_output_cmd [4afba402](https://github.com/ffplayout/ffplayout/commit/4afba4028aad488d404db9b09bac3166d7f33917)
- validate config regex

### ffpapi

- restructure api [ec4f5d2](https://github.com/ffplayout/ffplayout/commit/ec4f5d2ac23718aa6c3fc23f698f34a2e31b326b)
- import playlist from text file [#195](https://github.com/ffplayout/ffplayout/issues/195), [ec4f5d2](https://github.com/ffplayout/ffplayout/commit/ec4f5d2ac23718aa6c3fc23f698f34a2e31b326b)

### frontend

- style scrollbar on chrome browser [8be260a](https://github.com/ffplayout/ffplayout/commit/8be260ae207d33487f51ebd8f98eb26e16298bdb)

### Dokumentation

- add import example
- add new import cli parameter
- add doc for multiple audio outputs
- add info about experimental features

### Development

- use ffmpeg in action
- run tests only on Linux

## [0.15.0](https://github.com/ffplayout/ffplayout/compare/v0.15.0...v0.15.2) (2022-09-02)

### ffplayout

- validate file compression settings and filtering [9c51226](https://github.com/ffplayout/ffplayout/commit/9c5122696dc9065ff670c54abd0f87945b8865e1)
- fix length from filler clip in playlist generator [9c51226](https://github.com/ffplayout/ffplayout/commit/9c5122696dc9065ff670c54abd0f87945b8865e1)
- serialize values only when string is not empty [9c51226](https://github.com/ffplayout/ffplayout/commit/9c5122696dc9065ff670c54abd0f87945b8865e1)
- compare also audio and custom filter on playlist existing check [9c51226](https://github.com/ffplayout/ffplayout/commit/9c5122696dc9065ff670c54abd0f87945b8865e1)
- stop only when error comes not from hls segment deletion [a62c1d0](https://github.com/ffplayout/ffplayout/commit/a62c1d07c7e4f62ccd3e4158f6b5f50ee76a67cc)
- fix unwrap error on None output_cmd [7cd8789](https://github.com/ffplayout/ffplayout/commit/7cd87896a46833996986166dff7f89421b5cfb2d)

### ffpapi

- get UTC offset from system [6ff34e0](https://github.com/ffplayout/ffplayout/commit/6ff34e0ddb1940aeb7b69e4d6b6f35b348a6f541)

### frontend

- get UTC offset from API, fix [#182](https://github.com/ffplayout/ffplayout/issues/182)
- fix bugs related to time and playlist save [03aa2f3](https://github.com/ffplayout/ffplayout/commit/03aa2f3b01a79c93f650eeba6830be85d1293fec)
- add edit button to playlist items [03aa2f3](https://github.com/ffplayout/ffplayout/commit/03aa2f3b01a79c93f650eeba6830be85d1293fec)
- add custom filter to playlist item [03aa2f3](https://github.com/ffplayout/ffplayout/commit/03aa2f3b01a79c93f650eeba6830be85d1293fec)
- better responsive control [46140b4](https://github.com/ffplayout/ffplayout/commit/46140b42839485a37127a7add8818b7f6abf8417)
- remove perfect-scrollbar (use only browser scrollbar)
- fix logout button in menu
- remove escape character
- fix browser errors when engine is not running

### Dokumentation

- Fix spelling in Readme
- Add filtergraph/pipeline description
- Add complex custom filter example
