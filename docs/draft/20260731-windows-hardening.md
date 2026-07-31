# Launcher 요구사항

```text
function launchElectron():
    electronPath = resolveInstalledElectronPath()

    if not isExpectedInstallPath(electronPath):
        fail("INVALID_EXECUTABLE_PATH")

    if production and not verifyCodeSignature(electronPath):
        fail("INVALID_CODE_SIGNATURE")

    userSid        = getCurrentUserSid()
    localSystemSid = getLocalSystemSid()
    ownerRightsSid = getOwnerRightsSid()

    processDacl = createDacl([
        allow(userSid,
            SYNCHRONIZE |
            PROCESS_QUERY_LIMITED_INFORMATION |
            PROCESS_TERMINATE
        ),

        allow(localSystemSid, PROCESS_ALL_ACCESS),

        // 프로세스 소유자의 암묵적 WRITE_DAC 제거
        allow(ownerRightsSid, READ_CONTROL)
    ])

    initialThreadDacl = createDacl([
        allow(userSid,
            SYNCHRONIZE |
            THREAD_QUERY_LIMITED_INFORMATION
        ),

        allow(localSystemSid, THREAD_ALL_ACCESS),

        allow(ownerRightsSid, READ_CONTROL)
    ])

    mitigationPolicy = [
        DEP,
        ASLR,
        STRICT_HANDLE_CHECKS,
        EXTENSION_POINT_DISABLE,
        BLOCK_REMOTE_IMAGES,
        BLOCK_LOW_INTEGRITY_IMAGES,
        PREFER_SYSTEM32_IMAGES
    ]

    sessionNonce = secureRandom()

    sessionHandle = createInheritedOneTimeChannel(
        data = sessionNonce,
        dacl = currentUserOnly
    )

    startupInfo = createStartupInfoEx(
        mitigationPolicy = mitigationPolicy,
        inheritedHandles = [sessionHandle]
    )

    process = CreateProcess(
        executable = electronPath,
        flags = CREATE_SUSPENDED |
                EXTENDED_STARTUPINFO_PRESENT,
        processSecurity = processDacl,
        threadSecurity = initialThreadDacl,
        startupInfo = startupInfo
    )

    if process.creationFailed:
        fail("PROCESS_CREATION_FAILED")

    if not verifyProcessDacl(process.handle):
        terminate(process)
        fail("PROCESS_DACL_INVALID")

    if not verifyMitigationPolicy(process.handle):
        terminate(process)
        fail("MITIGATION_POLICY_INVALID")

    assignCompatibleJobObject(process)

    ResumeThread(process.initialThread)

    close(sessionHandle)
    close(process.initialThreadHandle)

    // CreateProcess가 반환한 강한 핸들을 장기간 유지하지 않음
    close(process.processHandle)

    zeroize(sessionNonce)
```

## Launcher가 보장해야 하는 것

```text
- Electron 프로세스 생성 시점부터 제한된 DACL 적용
- 현재 사용자에게 다음 권한을 허용하지 않음:
    PROCESS_VM_READ
    PROCESS_VM_WRITE
    PROCESS_VM_OPERATION
    PROCESS_CREATE_THREAD
    PROCESS_DUP_HANDLE
    PROCESS_QUERY_INFORMATION
    PROCESS_SUSPEND_RESUME
    WRITE_DAC
    WRITE_OWNER

- OWNER RIGHTS에는 READ_CONTROL만 허용
- mitigation은 CreateProcess 시점에 적용
- 상속 핸들은 allowlist 방식으로 제한
- Launcher가 받은 고권한 프로세스 핸들은 즉시 폐기
- 필수 정책 적용 실패 시 Electron을 실행하지 않음
```

# Hardening 요구사항

```text
function initializeHardening():
    if alreadyInitialized:
        return cachedStatus

    // vault, renderer, network 초기화보다 먼저 실행
    launcherSession = readInheritedOneTimeChannel()

    if launcherSession.missing:
        return failClosed("LAUNCHER_NOT_VERIFIED")

    if not validateLauncherSession(launcherSession):
        return failClosed("INVALID_LAUNCHER_SESSION")

    zeroize(launcherSession.secret)
    close(launcherSession.handle)

    processDacl = readCurrentProcessDacl()

    if not processDacl.allowsCurrentUserOnly([
        SYNCHRONIZE,
        PROCESS_QUERY_LIMITED_INFORMATION,
        PROCESS_TERMINATE
    ]):
        return failClosed("PROCESS_DACL_INVALID")

    if processDacl.allowsCurrentUserAny([
        PROCESS_VM_READ,
        PROCESS_VM_WRITE,
        PROCESS_VM_OPERATION,
        PROCESS_CREATE_THREAD,
        PROCESS_DUP_HANDLE,
        PROCESS_QUERY_INFORMATION,
        PROCESS_SUSPEND_RESUME,
        WRITE_DAC,
        WRITE_OWNER
    ]):
        return failClosed("DANGEROUS_PROCESS_ACCESS")

    if not processDacl.ownerRightsEquals(READ_CONTROL):
        return failClosed("OWNER_RIGHTS_INVALID")

    runtimePolicies = [
        EXTENSION_POINT_DISABLE,
        BLOCK_REMOTE_IMAGES,
        BLOCK_LOW_INTEGRITY_IMAGES,
        PREFER_SYSTEM32_IMAGES
    ]

    for policy in runtimePolicies:
        result = applyOrVerify(policy)

        if policy.required and result.failed:
            return failClosed("MITIGATION_FAILED")

    if not restrictDllSearchPath([
        APPLICATION_DIRECTORY,
        SYSTEM32
    ]):
        return failClosed("DLL_SEARCH_POLICY_FAILED")

    cachedStatus = {
        state: SECURE,
        vaultUnlockAllowed: true
    }

    return cachedStatus
```

## Hardening이 보장해야 하는 것

```text
- Electron main의 가장 첫 native 작업으로 실행
- Launcher를 거친 실행인지 검증
- 현재 프로세스 DACL이 예상대로 적용됐는지 검증
- OWNER RIGHTS가 WRITE_DAC을 갖지 않는지 검증
- 위험한 프로세스 접근 권한이 허용되지 않았는지 검증
- 런타임에 적용 가능한 mitigation 적용 및 재검증
- DLL 검색 경로 제한
- 실패 시 vault backend 초기화 및 unlock 차단
- nonce, SID, 핸들, 메모리 주소를 JS나 로그에 노출하지 않음
- 여러 번 호출되어도 같은 결과를 반환하도록 멱등성 보장
```

# 실행 순서

```text
launcher.exe
    → 제한된 DACL과 mitigation으로 electron.exe 생성
    → electron.exe 시작
    → hardening.node.initialize()
    → 검증 성공
    → vault backend 초기화
    → renderer 생성
```

