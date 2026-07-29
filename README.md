## `keeless`
**minimal password manager**

> [!warning]
> **AI-Generated code ahead**  
> It is generally a bad idea to use AI-generated code for something as sensitive as a password manager.  
> This project was built purely for my personal use and learning.
> If you decide to use it, you are doing so entirely at your own risk!

## Features
* 🗝️ **KDBX 3.1 / 4.1 Support**  
  It uses the KDBX3.1/4.1 file format and compatible with multiple keepass implementations.
  
* 🔌 **Flexible Topology**  
  It compiles to both wasm and native and can host in web, browser extensions, and desktop (Windows / Linux) environments.
  
* 🪪 **Multi-Credentials**  
  Besides traditional passwords, passkeys and TOTP are supported.  
  On Linux, it supports native Passkey authentication through a virtual HID.
  
* 🔄 **Synchronization**  
  WebDAV and local file systems are supported. Synchronization is implemented using CAS and 3-way merging.
  
* ✨ **Modern UI**  
  I personally don't like the design of other KeePass desktop apps, so I put some efforts on design.

## Status
**`Working On My Machine™`**  
> If you intend to use this, please backup your database and expect data loss.

## TODO
- [x] enforce_auto_lock을 dispatch 시에 구현하지말고 자체 timeout으로 작업하게
- [x] desktop용 cache + journal로 저장하는 storage wrapper
- [x] vhid 구현 (Linux)
- [x] DB 내보내기 기능
- [x] 설정 UI
- [ ] 엔트리 검색 로직 러스트로 일원화 [skim](https://github.com/skim-rs/skim)사용, `tag:` 쿼리, `in:` 쿼리
- [ ] WebDAV 싱크 테스트 하기
- [ ] Attachment 다운로드 가능
- [ ] setuplayout에서 위에 <- Back으로 하게, router로 이동하게
- [ ] per-database config store, per-database lesswire key upgrade
- [ ] scoped approved lesswire key (app, passkey, extension)
- [ ] Windows WebAuthn plugin authenticator 구현
- [ ] extension + native messaging host 구현
- [ ] 화면 캡쳐 방어
- [ ] SetSecurityInfo (메인 프로세스 / 렌더러 프로세스 원격 스레드, 덤프 차단)
- [ ] auto save 및 dirty status 보여주기
- [ ] desktop에서 register client가 blindly register 시키는 것 막기
  - renderer에 otp 주고, 그 otp 기반으로 이전의 client revoke시키고 새 client로 등록, otp reuse 시 전체 revoke

## Installation

## Screenshot

## Security
#### Encrypted Memory
There is very little that we can do once admin permissions are stolen. We can only offer best-effort protection.  
The master key is encrypted and protected from being written out to the disk, and is zeroized as quickly as possible.  
Most entry keys are also stored in an encrypted state. But all entries are temporarily decrypted while doing a search,
url matching (look for `MemoryUnlockSession` in the code).  

Also, individual entries may live long in the JavaScript memory when viewing, copying or autofilling a password,
leaving it vulnerable to memory dumps as well.

#### Paranoia Mode
For users who want extreme security, this mode ensures no passwords are kept in memory, prompting the user
for the password on every synchronization attempt.
However, even with Paranoia mode enabled, individual entries can also live long in the JavaScript memory.  
Again, once admin permissions are stolen, there is truly very little that can be done.

#### Native Master Key Input
In the desktop app, master key inputs bypass the web renderer (Tauri WebView) and are handled using `egui`,
which helps the master key to be easily zeroized.

#### Signed IPC Protocol
All messages between the clients (App/Extension) and the core are signed and encrypted.  
Connecting the companion web extension is based on tofu(trust on first use).
An approval dialog ensures that only explicitly authorized devices can establish a connection and access the database.

## FAQ
> Not actually "frequently asked", but rather what I expect to be frequently asked.

* **Is it safe?**  
  At least I roughly audited the code myself, but you have no reason to trust me, so read the source, Luke.

* **Why did you create this?**  
  Why not?  
  When I looked at existing desktop KeePass clients, it turned out that KeeWeb has way too outdated electron (which does not support wayland),
  and KeePassXC does not have WebDAV support (+ it just didn't look very nice for me, in terms of aesthetics, honestly).  
  Plus, as GPT-5.6 Sol gave me many resets (Thank you Tibo), I really wanted to give this whole vibe-coding thing a try.

* **I like the design/features, but I'm not sure about the results of vibe coding.**  
  In fact, most of the architectural design was done by me, so it is not actually 100% vibe coded.
  But anyway if you don't want those, that's where the open-source comes into play.
  As the backend and the frontend are strictly separated, you can freely fork and create your own frontend/backend implementation.  
  If you rewrite the backend, please let me know so I can use your implementation.

* **Why are you using Electron? Electron is heavy and &#35;&#36;&#33;&#35;&#36;&#35;&#36;&#64;**  
  The initial version was built using Tauri but I had a hard time with WebkitGTK.  
  Sorry Windows users, you were sacrificed for the Linux users. Though, as always, you can fork!

* **Can I contribute using AI?**  
  In Korea, there's a term "naeronambul", which roughly translates to "For me, it's romance. For you, it's cheating".
  So, basically, no.  
  Buuuuut, if you have a < 1.5k LOC diff and if you can write the description in your own words, you can give it a shot.
  However, if I smell any slop in the code, I can close it without any further comments.

* **Can you guarantee that the memory protection thing works well?**  
  No, I can not. But I tested for a few basic smoke tests manually on Windows.
  If you are concerned about it, just test it yourself and share the results with me via issues.
