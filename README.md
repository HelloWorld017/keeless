## `keeless`

> [!warning]
> **AI-Generated code ahead**  
> It is generally a bad idea to use AI-generated code for something as sensitive as a password manager.  
> This project was built purely for my personal use and learning.
> If you decide to use it, you are doing so entirely at your own risk!

## Features
* **KDBX4 Support**  
  It uses the KDBX4 file format and compatible with multiple keepass implementations.
  
* **Flexible Topology**  
  It compiles to both wasm and native and can host in web, browser extensions, and desktop (Windows / Linux) environments.
  
* **Multi-Credentials**  
  Besides traditional passwords, passkeys and TOTP are supported.  
  On Linux, it supports background Passkey authentication through a virtual HID.
  
* **Synchronization**  
  WebDAV and local file systems are supported. Synchronization is implemented using CAS and 3-way merging.
  
* **Modern UI**  
  I put some efforts on design.

## Installation

## Screenshot

## Security
* **Memory Protection**  
  There is very little that we can do once admin permissions are stolen. We can only offer best-effort protection.
  The master key is encrypted and protected from being written out to the disk, and is zeroized as quickly as possible.
  Most entry keys are also stored in an encrypted state. But all entries are temporarily decrypted while doing a search,
  url matching (look for `MemoryUnlockSession` in the code).
  Also, individual entries may live long in the JavaScript memory when viewing, copying or autofilling a password,
  leaving it vulnerable to memory dumps as well.
  
* **Paranoia Mode**  
  For users who want extreme security, this mode ensures no passwords are kept in memory, prompting the user
  for the password on every synchronization attempt.
  However, even with Paranoia mode enabled, individual entries can also live long in the JavaScript memory.
  Again, once admin permissions are stolen, there is truly very little that can be done.
  
* **Secure Master Key Input**  
  In the desktop app, master key inputs bypass the web renderer (Tauri WebView) and are handled using `egui`.
  Which can help the master key to be zeroized.
  
* **Signed IPC Protocol**  
  All messages between the clients (App/Extension) and the core are signed and encrypted.
  Connecting the companion web extension is based on tofu(trust on first use).
  Access from unregistered keys Devices) is immediately dropped.
  An approval dialog ensures that only explicitly authorized devices can establish a connection and access the database.
