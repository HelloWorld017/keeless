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
  There are only few things we can do if the admin permission is stolen. Only best efforts are guaranteed.  
  The master key is encrypted and protected from being written out to disk, zeroized as soon as possible.
  
* **Paranoia Mode**  
  For users who want extreme security, this mode ensures no passwords are ever kept in memory, prompting the user for the password on every synchronization attempt.
  
* **Secure Master Key Input**  
  In the desktop app, master key inputs bypass the web renderer (Tauri WebView) and are handled using `egui`.
  
* **Signed IPC Protocol**  
  All messages between the clients (App/Extension) and the core are signed and encrypted.
  
* **Approval-based Handshake**  
  Connecting the companion web extension is based on tofu(trust on first use).
  Access from unregistered keys Devices) is immediately dropped. An approval dialog ensures that only explicitly authorized devices can establish a connection and access the database.
