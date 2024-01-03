# Changelog

## [0.20.3](https://github.com/ffplayout/ffplayout/releases/tag/v0.20.3) (2024-01-03)

### ffplayout

- improve live sources [9912405](https://github.com/ffplayout/ffplayout/commit/9912405e4e976b99be9d174fa9cc54700984d5a9)
- update sysinfo to support stats on network storage [8737769](https://github.com/ffplayout/ffplayout/commit/873776908e10b2eb9d92fb743a578a848e95c49c)

### Documentation

- fix API examples [c8ca4588d](https://github.com/ffplayout/ffplayout/commit/c8ca4588d178b1f94f5c7dce40fd4a07a10a695b)

## [0.20.2](https://github.com/ffplayout/ffplayout/releases/tag/v0.20.2) (2023-12-16)

### ffplayout

- better error message [5c14b89](https://github.com/ffplayout/ffplayout/commit/5c14b895f2c8e34990097354fea860a5030a5732)
- warn and adjust duration on validation [a30f21b](https://github.com/ffplayout/ffplayout/commit/a30f21b86688fbf4de477279217ca3a739409719)

### ffpapi

- thread block on hashing [4c4199cb](https://github.com/ffplayout/ffplayout/commit/4c4199cbdb0836d69d67fd6dee1869fb08eeffbf)
- remove salt from table [15f41148](https://github.com/ffplayout/ffplayout/commit/15f41148dfb26ccaea159f5c5305a966cf81b1c4)

### frontend

- possibility to preview live/html sources [5881527](https://github.com/ffplayout/ffplayout/pull/472/commits/5881527fc571feccaee7f7f1877750ccc44516f5)

## [0.20.1](https://github.com/ffplayout/ffplayout/releases/tag/v0.20.1) (2023-12-03)

### ffplayout

- add silence detection for validation [ea83160](https://github.com/ffplayout/ffplayout/commit/ea83160ba63bb8723de1f004f6449b37a1ea2593)
- loop separate audio when is to short [94e02ac](https://github.com/ffplayout/ffplayout/commit/94e02ac3678c0f8cdec97002f30e08beb45e748b)
- add probe object in validation thread, to hopefully reduce latency and reduce unneeded file access [0330ad6](https://github.com/ffplayout/ffplayout/commit/0330ad61681a4cb576d4a46365c8cdffdfc96379)

### ffpapi

- update actix-web-grants to v4 [f1e87975](https://github.com/ffplayout/ffplayout/commit/f1e8797528e649aac6de85d897b7c03b8007a2b3)

### frontend

- call system status only when app is not hidden [3f22297](https://github.com/ffplayout/ffplayout/commit/3f222975c16580deeeedaa2e0721e4a312e7c8fb)
- select, edit and delete user [f86a6c3](https://github.com/ffplayout/ffplayout/commit/f86a6c3f1dfb8ec5f3c8e74714b8eecda2b443c3)
- global middleware [c60d60d](https://github.com/ffplayout/ffplayout/commit/c60d60d9b3f74095034760f22876aed877e0464f)

## [0.20.0](https://github.com/ffplayout/ffplayout/releases/tag/v0.20.0) (2023-11-16)

### ffplayout

- run task on clip change, #276 [5bd1b2](https://github.com/ffplayout/ffplayout/commit/5bd1b23513d3cb0a9f6574626032acdd6627e790)
- support filler folder [98d1d5](https://github.com/ffplayout/ffplayout/commit/98d1d5d606b3f90ebeb1f0cd54156ee820272dd2) [04353a](https://github.com/ffplayout/ffplayout/commit/04353a984d43e1059ee9808ee08700e8c5e1cb8b)
- support log level as cmd argument [334f84](https://github.com/ffplayout/ffplayout/commit/334f842d1923e7150f0ed504fa85f4936c0213d7)
-  add stream copy mode, fix #324 [b44efd](https://github.com/ffplayout/ffplayout/commit/b44efde8f1a771122c10f79e1a5da8ba724acd56)
- replace realtime filter with readrate parameter for hls mode [4b18d41](https://github.com/ffplayout/ffplayout/commit/4b18d414b7437f48a3663e9e9b547e83ab605cda) (**!WARNING:** older ffmpeg versions will not work anymore! Now 5.0+ is needed.)
- choice audio track index, fix #348 [1bfff2](https://github.com/ffplayout/ffplayout/commit/1bfff27b4b46405b52a428b38bd00fe4e9c3f78d)
- fix boxborderw value [fef7d0](https://github.com/ffplayout/ffplayout/commit/fef7d04e65b6275b6bb6c5b813c83b8641051882)
- stop decoder with SIGTERM signal, instead of kill on non windows systems [d2c72d](https://github.com/ffplayout/ffplayout/commit/d2c72d56fe0cc1cced14f8d1d1746f5224011499)
- generate playlists based on template [0c51f8](https://github.com/ffplayout/ffplayout/commit/0c51f8303cd3eacdec8a0ac3abe9edd69e2271c2)
- update chrono and fix audit warning [83cff6](https://github.com/ffplayout/ffplayout/commit/83cff609b3709f4621af506de2f8546099b8848c)
- jump out from source loop when playout is terminated [cf6e56](https://github.com/ffplayout/ffplayout/commit/cf6e5663e98eb52bc84c0e9e5856943ddefc24d9)
- fix program hang when mail sending not work [38e73a](https://github.com/ffplayout/ffplayout/commit/38e73a0138430fc600ae809356127941e1f08eb2)

### ffpapi

- embed static files from frontend in ffpapi, add db path argument [b4cde6e](https://github.com/ffplayout/ffplayout/commit/b4cde6e12ce70af20f52f308d7cb4288f97d31fe)
- Use enum for Role everywhere [7d31735](https://github.com/ffplayout/ffplayout/commit/7d3173533fd8b2a9d6e718ada0c81f017aedc777)
- get config also as normal user [7d31735](https://github.com/ffplayout/ffplayout/commit/7d3173533fd8b2a9d6e718ada0c81f017aedc777)
- fix time shift [7d31735](https://github.com/ffplayout/ffplayout/commit/7d3173533fd8b2a9d6e718ada0c81f017aedc777)
- add option for public path [c304386](https://github.com/ffplayout/ffplayout/commit/c30438697d33fe360e92146c03ad8ce212e138a6)
- add system stat route [c304386](https://github.com/ffplayout/ffplayout/commit/c30438697d33fe360e92146c03ad8ce212e138a6)

### frontend

- option to add user [debb75](https://github.com/ffplayout/ffplayout/commit/debb751428239f2d0ac446a0b9a805cd1ec4a965)
- fix audit alert, get status from playout stat [50bee9](https://github.com/ffplayout/ffplayout-frontend/commit/50bee93c8555b14181864a654239f7e68c50cafb)
- restart modal for config save [2f3234](https://github.com/ffplayout/ffplayout-frontend/commit/2f3234221a0aef8e70d9e2b5e9bbfb1fe51921fc)
- add advanced playlist generator, update packages [806d53](https://github.com/ffplayout/ffplayout-frontend/commit/806d533bc2a84fc994897371071c4399172fa639)
- add dashboard [ba0c0fa](https://github.com/ffplayout/ffplayout/pull/446/commits/ba0c0faaac9c44fbf4f87752c89aaa8859be9bf1)


## [0.19.1](https://github.com/ffplayout/ffplayout/releases/tag/v0.19.1) (2023-10-08)

### ffplayout

- remove openssl dependencies [813e48f](https://github.com/ffplayout/ffplayout/commit/813e48fd54a6482eb09ec418e507733d689663d9)
- update packages [0808fb](https://github.com/ffplayout/ffplayout/commit/0808fb29ab8db17cf1d251336cc90c1db7aa92e0)

### frontend

-  fix preview in player #397 [943cf9](https://github.com/ffplayout/ffplayout/commit/943cf90e15edc0efdb9abf0703cc6addbd3dfecc)

## [0.19.0](https://github.com/ffplayout/ffplayout/releases/tag/v0.19.0) (2023-07-19)

### ffplayout

- cleanup and update docker files, migrate to notify 6.0 [5502c45](https://github.com/ffplayout/ffplayout/commit/5502c45420a12b63c05493b2c69d4b6cdd0b044e)
- switch jsonrpc-http-server to tiny_http, update clap to next major version [8eb5c2b](https://github.com/ffplayout/ffplayout/commit/8eb5c2ba0280eeed25231e3379c88a9bfb47334c) [2b4fbff](https://github.com/ffplayout/ffplayout/commit/2b4fbff2dcbb23714b2fd851931df9c0fa15221c)
    - The jsonrpc-http-server don't get any updates anymore and some libs are already unmaintained. Migration to the new jsonrpsee makes not so much sense, because its features are not needed. For our needs tiny_http is absolut enough.
- set chrono features, cleanup, less logging [4a578e8](https://github.com/ffplayout/ffplayout/commit/4a578e83ffd4a8897521c45c5d1804eb961fec72)
- deserialize numbers to string for drawtext filter [c02241f](https://github.com/ffplayout/ffplayout/commit/c02241ffe8126e761ba9440c41e2d2f181ca40ea)
- add doc strings to rpc server [25e2ed7](https://github.com/ffplayout/ffplayout/commit/25e2ed739091f4de444110cdaf6f639b14397e86) [7c398c5](https://github.com/ffplayout/ffplayout/commit/7c398c5e556ca00140080bbe9fd4f424fe8d867a)
- run service inside docker as root, fix #329 [c4d5aec](https://github.com/ffplayout/ffplayout/commit/c4d5aec63e81db7706e21e7b4f7198073008538e)
- add duration from remote source, #336 [a15c8a0](https://github.com/ffplayout/ffplayout/commit/a15c8a01ba05749036048bda26ccb3918e1ce7af)
- don't log missing source when playlist is to short add validate playlist option [83432e](https://github.com/ffplayout/ffplayout/commit/83432ef6735c5058a2251f76e0cee51d323ec774)
- debug log config path [40fd1c4](https://github.com/ffplayout/ffplayout/commit/40fd1c4751f46ae3630965095329a7832548d304)
- check if json rpc port is in use [ac90dcb](https://github.com/ffplayout/ffplayout/commit/ac90dcb157784a3b98990140f5535622a6689e65)
- fix ffmpeg zombies in HLS mode [972567a](https://github.com/ffplayout/ffplayout/commit/972567afa6e0b868e5114c40b00c7a620014d09a)

### ffpapi

- update sqlx to 0.7 [cd4c872](https://github.com/ffplayout/ffplayout/commit/cd4c8727bd0e908eb3f23e73b35f56ccda5938d1)
- rename hls output, fix #351 [acfe223](https://github.com/ffplayout/ffplayout/commit/acfe223301fd3d70cc358159dff122fc149bc32e)

### frontend

- fix empty remote names [968de86](https://github.com/ffplayout/ffplayout/commit/968de862f4d4f3348125fc5bd1be60f0cbcb6627)
- fix type errors [eca9507](https://github.com/ffplayout/ffplayout/commit/eca9507a1fc9c40c4de9c368ec72fd4a90e82c12)
- fix http-flv player, #349  [bf993a1](https://github.com/ffplayout/ffplayout/commit/bf993a13329204f74d50fd405afbe900859b4a95)
- watch channel change on player page, #351 [50204ce](https://github.com/ffplayout/ffplayout/commit/50204ce3815d52214a426ed3e93178117ad3be2c)
- update nuxtjs to 3.6.3 [5dd450e](https://github.com/ffplayout/ffplayout/pull/358/commits/5dd450e90c2151ebc37447c8659251c344da75be)

### Development

- init or update submodules [cd8a039](https://github.com/ffplayout/ffplayout/commit/cd8a039a6d7873eea6456564dff9ea3244005457)

### Documentation

- format text from Readme [26a7ac0](https://github.com/ffplayout/ffplayout/commit/26a7ac02b06cf2094f39c4ed5ce7990f83d69c28)
- simplify preview streaming example [6ca710d](https://github.com/ffplayout/ffplayout/commit/6ca710ded68e107acdb47f029ba4d0f33460ac2b)

## [0.18.4](https://github.com/ffplayout/ffplayout/releases/tag/v0.18.4) (2023-06-25)

### ffplayout

- fix player control in HLS Mode [ec33cdb](https://github.com/ffplayout/ffplayout/commit/ec33cdb30944ab19c028a085fcb6d974ec4e81be)


## [0.18.3](https://github.com/ffplayout/ffplayout/releases/tag/v0.18.3) (2023-06-16)

### ffpapi

-  remove extra content type from header, fix [#331](https://github.com/ffplayout/ffplayout/issues/331)

## [0.18.2](https://github.com/ffplayout/ffplayout/releases/tag/v0.18.2) (2023-06-13)

### ffplayout

- update version, create dir with ignore error [2da9d1a](https://github.com/ffplayout/ffplayout/pull/327/commits/2da9d1a85d7ca3695022a74d79cf362a10e19705)
- add postrm, fix #326 [97455d5](https://github.com/ffplayout/ffplayout/pull/328/commits/97455d535c6214b04eca14812029ced23c7524e1)

## [0.18.1](https://github.com/ffplayout/ffplayout/releases/tag/v0.18.1) (2023-06-11)

### frontend

- update bootstrap to stable version [7f10e90](https://github.com/ffplayout/ffplayout/pull/325/commits/7f10e9013aabd44cb5d01193db4b10b0884c0cb3)
- fix config save [abf3d89](https://github.com/ffplayout/ffplayout/pull/325/commits/abf3d897a1df1fa35f06362f2e26d4ae1217bda4)
- hide chunk size waring [63d2849](https://github.com/ffplayout/ffplayout/pull/325/commits/63d28494d5ba263cf7e26d2022f990190cb8f6c2)

### ffplayout

- update packages [7f10e90](https://github.com/ffplayout/ffplayout/pull/325/commits/7f10e9013aabd44cb5d01193db4b10b0884c0cb3) [8dd8865](https://github.com/ffplayout/ffplayout/pull/325/commits/8dd886547bad342b8c000a63c025419583d8003f)
- remove redundant clone [d6baccf](https://github.com/ffplayout/ffplayout/pull/325/commits/d6baccf3a7ff645a8ca2938d782f2eee0ec08eb3)

## [0.18.0](https://github.com/ffplayout/ffplayout/releases/tag/v0.18.0) (2023-05-28)

### frontend

- mark and scroll to current clip, show when ingest is running [676d71e](https://github.com/ffplayout/ffplayout/commit/676d71e9b7ca37b1b40f1007f242023d49eed63b)
- split extensions to array, fix #318 [5871d09](https://github.com/ffplayout/ffplayout/commit/5871d092af020c278f267a57ef49c592f39ecd79)

### ffplayout

- remove loudnorm filter [535511f](https://github.com/ffplayout/ffplayout/commit/535511f394a98441be15fc62090340e94b2f5018)
    - quality is to bad
- no regex match validation for scale filter [d1ce475](https://github.com/ffplayout/ffplayout/commit/d1ce4756924e4cfc969db91adaadfcd88c195dd0)
- try to create log path, if not exists. expose state file in config (important for multi channels) [6cd092c](https://github.com/ffplayout/ffplayout/commit/6cd092c30fd7c22428c0c0792987ef419a781ff5)

### ffpapi
- update most importend config values on new channel [6338207](https://github.com/ffplayout/ffplayout/commit/6338207fba9f217f144cb75afc764c16e5e3223e)


## [0.17.1](https://github.com/ffplayout/ffplayout/releases/tag/v0.17.1) (2023-04-07)

### frontend
- fix upload function [5e976f2](https://github.com/ffplayout/ffplayout/pull/310/commits/5e976f212b47d572839e01ee73dfb632fbe1a70c)
- update bootstrap to 5.3.0 alpha 3 [8024a99](https://github.com/ffplayout/ffplayout/pull/310/commits/8024a990a651920ba2244f6b120ecff9701c79d2)

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
