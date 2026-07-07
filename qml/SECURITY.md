# QML Security Guide

This document outlines the security measures, testing procedures, and best practices for the QML layer of the Mukei application.

## Overview

The QML layer is designed with security as a primary concern. All sensitive operations are delegated to the C++ layer through controlled bridges, minimizing the attack surface within the QML runtime environment.

## Security Architecture

### Controlled Bridges

All system-level operations flow through explicitly defined C++ bridges:

- **mukeiClipboard**: Clipboard access (implemented in `main.cpp`)
- **mukeiAgent**: AI agent communication
- **mukeiBridge**: Platform-specific functionality
- **safRegistry**: Security access token management

### No Direct System Access

QML components have no direct access to:
- File system (except through FileDialog with user consent)
- Network (all HTTP requests go through C++ layer)
- System clipboard (controlled via mukeiClipboard bridge)
- Environment variables
- Native APIs

## Implemented Security Measures

### 1. Clipboard Bridge Implementation

**File**: `main.cpp`

```cpp
class MukeiClipboard final : public QObject
{
    Q_OBJECT
public:
    Q_INVOKABLE void setText(const QString &text);
    Q_INVOKABLE QString text() const;
};
```

**Usage in QML**:
```qml
onClicked: {
    mukeiClipboard.setText(root.textToCopy);
}
```

### 2. No Console Logging in Production

All console logging has been removed from production code. The `CopyButton` component previously logged warnings when the clipboard bridge was unavailable - this has been eliminated.

**Before**:
```qml
} else {
    console.warn("CopyButton: mukeiClipboard bridge unavailable, no-op");
}
```

**After**:
```qml
// Direct call - bridge is guaranteed to exist
mukeiClipboard.setText(root.textToCopy);
```

### 3. Static Resource Loading

All fonts and icons are loaded from Qt Resource Collection (QRC) files:

```qml
iconSource: "qrc:/icons/check.svg"
source: "qrc:/fonts/Inter-Variable.ttf"
```

No external URLs or dynamic resource loading is permitted.

### 4. Controlled Dynamic Object Creation

`Qt.createQmlObject()` is only used for:
- Font loaders with static, bundled resources
- Test fixtures

Never with user-provided input or string interpolation.

## Security Testing

### Integration Tests

**File**: `tests/tst_Security.qml`

Run tests:
```bash
cd qml/build
./tst_Security
```

Tests verify:
- ✓ Clipboard bridge exists and is functional
- ✓ No console logging occurs during normal operation
- ✓ Components maintain isolation
- ✓ Accessible properties are properly set
- ✓ No global scope pollution

### Static Analysis

**File**: `scripts/qml_security_analyzer.py`

Run analysis:
```bash
python3 scripts/qml_security_analyzer.py qml
```

Detects:
- Console logging statements
- Dynamic code execution patterns (`eval`, `Function` constructor)
- Unsafe imports
- Network access from QML
- File system operations
- External resource loading

### CI/CD Integration

Security checks run automatically on every PR:

**Workflow**: `.github/workflows/qml-check.yml`

Jobs:
1. **icon-uniqueness**: Prevents duplicate icon payloads
2. **fonts-are-real**: Validates TTF magic bytes
3. **qrc-cross-reference**: Ensures all resource paths resolve
4. **ddg-regression**: Guards against forbidden search providers
5. **qmllint**: Static QML syntax validation
6. **cmake-configure**: Build configuration validation
7. **qml-security-tests**: Runtime security test suite
8. **qml-static-security-analysis**: Pattern-based security scanning

## Security Best Practices

### DO:
- Use bridges for all system operations
- Load resources from QRC paths only
- Validate all user input before passing to C++
- Keep QML logic minimal and declarative
- Use property bindings instead of imperative code
- Test with accessibility tools

### DON'T:
- Use `console.log/warn/error` in production code
- Call `eval()` or `Function()` constructors
- Load external URLs directly in QML
- Access file system without user consent dialogs
- Store sensitive data in QML properties
- Use `Qt.labs` modules without security review

## Vulnerability Response

### Reporting

Report security vulnerabilities through the project's security channel (see `SECURITY.md`).

### Response Process

1. **Triage**: Assess severity within 24 hours
2. **Analysis**: Determine root cause and scope
3. **Remediation**: Develop and test fix
4. **Disclosure**: Coordinate responsible disclosure

### Common Vulnerabilities and Mitigations

| Vulnerability | Mitigation |
|--------------|------------|
| XSS via user input | Sanitize in C++ layer before display |
| Information leakage | Remove console logging, use proper error handling |
| Clipboard injection | Validate content before setting clipboard |
| Resource confusion | Use QRC paths, validate external resources |

## Audit Checklist

Before each release, verify:

- [ ] No console logging in production code
- [ ] All system access goes through bridges
- [ ] No dynamic code execution with user input
- [ ] All resources load from QRC
- [ ] Security tests pass
- [ ] Static analysis shows no critical issues
- [ ] Dependencies are up to date
- [ ] No new `Qt.labs` imports without review

## Tools and Resources

- **qmllint**: Qt's built-in QML linter
- **qml_security_analyzer.py**: Custom security scanner
- **Qt Test Framework**: QML test infrastructure
- **CMake**: Build system with security flags

## Related Documentation

- `SECURITY.md`: Overall project security policy
- `README.md`: Project overview
- `main.cpp`: Bridge implementations
- `components/CopyButton.qml`: Example secure component

---

*Last updated: 2025*
*Version: 1.0*
