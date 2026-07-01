#!/usr/bin/env python3
"""
QML Static Security Analyzer

Performs static analysis on QML files to detect potential security issues:
- Console logging statements (console.log, console.warn, console.error)
- Dynamic code execution (Qt.createQmlObject with user input)
- Unsafe URL loading (XmlHttpRequest, WebEngine)
- File system access patterns
- Eval-like patterns
- External resource loading without validation

Usage:
    python qml_security_analyzer.py [qml_directory]
    
Returns exit code 0 if no critical issues found, 1 otherwise.
"""

import os
import re
import sys
from pathlib import Path
from typing import List, Tuple, Dict

# Security issue severity levels
CRITICAL = "CRITICAL"
HIGH = "HIGH"
MEDIUM = "MEDIUM"
LOW = "LOW"
INFO = "INFO"

class SecurityIssue:
    def __init__(self, file: str, line: int, severity: str, category: str, message: str, code_snippet: str = ""):
        self.file = file
        self.line = line
        self.severity = severity
        self.category = category
        self.message = message
        self.code_snippet = code_snippet
    
    def __str__(self):
        return f"[{self.severity}] {self.file}:{self.line} - {self.category}: {self.message}"

class QMLSecurityAnalyzer:
    def __init__(self):
        self.issues: List[SecurityIssue] = []
        
        # Patterns for detecting security issues
        self.patterns = {
            # Console logging - can leak information in production
            'console_logging': {
                'pattern': r'\bconsole\.(log|warn|error|debug|info)\s*\(',
                'severity': LOW,
                'message': 'Console logging detected - remove before production'
            },
            
            # Dynamic code execution - high risk if user input is involved
            'dynamic_code': {
                'pattern': r'\bQt\.createQmlObject\s*\([^)]*[\+\$]',
                'severity': HIGH,
                'message': 'Dynamic QML object creation with potential string interpolation'
            },
            
            # Eval-like patterns
            'eval_usage': {
                'pattern': r'\beval\s*\(',
                'severity': CRITICAL,
                'message': 'Eval usage detected - arbitrary code execution risk'
            },
            
            # XmlHttpRequest - network access from QML
            'xml_http_request': {
                'pattern': r'\bXmlHttpRequest\b',
                'severity': MEDIUM,
                'message': 'XmlHttpRequest detected - consider using C++ network layer'
            },
            
            # WebEngine/Webview - embedded browser risks
            'webengine': {
                'pattern': r'\bWebEngineView\b|\bWebView\b',
                'severity': HIGH,
                'message': 'WebEngine/WebView detected - XSS and injection risks'
            },
            
            # LocalStorage - persistent client-side storage
            'localstorage': {
                'pattern': r'\bLocalStorage\b',
                'severity': MEDIUM,
                'message': 'LocalStorage usage - ensure data is not sensitive'
            },
            
            # File system access
            'file_dialog': {
                'pattern': r'\bFileDialog\b',
                'severity': MEDIUM,
                'message': 'FileDialog detected - validate file paths'
            },
            
            # Unsafe external resource loading
            'external_loader': {
                'pattern': r'source:\s*["\']https?://',
                'severity': HIGH,
                'message': 'External HTTP/HTTPS resource loading - validate URLs'
            },
            
            # Function constructor (like eval)
            'function_constructor': {
                'pattern': r'\bFunction\s*\(',
                'severity': CRITICAL,
                'message': 'Function constructor detected - arbitrary code execution risk'
            },
            
            # setTimeout/setInterval with string (like eval)
            'timer_with_string': {
                'pattern': r'\b(setTimeout|setInterval)\s*\(\s*["\']',
                'severity': HIGH,
                'message': 'Timer with string argument - use function instead'
            }
        }
        
        # Additional context-aware checks
        self.context_checks = [
            self.check_unsafe_imports,
            self.check_clipboard_patterns,
            self.check_network_access,
            self.check_file_operations
        ]
    
    def analyze_file(self, filepath: Path) -> List[SecurityIssue]:
        """Analyze a single QML file for security issues."""
        file_issues = []
        
        try:
            with open(filepath, 'r', encoding='utf-8') as f:
                lines = f.readlines()
        except Exception as e:
            return [SecurityIssue(
                str(filepath), 0, HIGH, "File Access",
                f"Could not read file: {e}"
            )]
        
        # Pattern-based checks
        for line_num, line in enumerate(lines, 1):
            for pattern_name, pattern_info in self.patterns.items():
                if re.search(pattern_info['pattern'], line):
                    file_issues.append(SecurityIssue(
                        file=str(filepath),
                        line=line_num,
                        severity=pattern_info['severity'],
                        category=pattern_name,
                        message=pattern_info['message'],
                        code_snippet=line.strip()[:100]
                    ))
        
        # Context-aware checks
        content = ''.join(lines)
        for check_func in self.context_checks:
            context_issues = check_func(filepath, content, lines)
            file_issues.extend(context_issues)
        
        return file_issues
    
    def check_unsafe_imports(self, filepath: Path, content: str, lines: List[str]) -> List[SecurityIssue]:
        """Check for potentially unsafe module imports."""
        issues = []
        
        unsafe_imports = [
            ('Qt.labs.settings', 'Settings persistence - validate stored data'),
            ('Qt.labs.platform', 'Platform-specific features - review permissions'),
        ]
        
        for module, message in unsafe_imports:
            if module in content:
                # Find the line
                for line_num, line in enumerate(lines, 1):
                    if module in line:
                        issues.append(SecurityIssue(
                            file=str(filepath),
                            line=line_num,
                            severity=INFO,
                            category='unsafe_import',
                            message=f"{module}: {message}"
                        ))
                        break
        
        return issues
    
    def check_clipboard_patterns(self, filepath: Path, content: str, lines: List[str]) -> List[SecurityIssue]:
        """Check clipboard usage patterns."""
        issues = []
        
        # Check for clipboard access without proper bridges
        if 'Clipboard' in content and 'mukeiClipboard' not in content:
            issues.append(SecurityIssue(
                file=str(filepath),
                line=0,
                severity=MEDIUM,
                category='clipboard_access',
                message='Direct Clipboard access detected - use controlled bridge instead'
            ))
        
        return issues
    
    def check_network_access(self, filepath: Path, content: str, lines: List[str]) -> List[SecurityIssue]:
        """Check for network access patterns."""
        issues = []
        
        # Check for direct network access
        network_patterns = [
            r'\bXMLHttpRequest\b',
            r'\bWebSocket\b',
            r'\bHttpServer\b'
        ]
        
        for pattern in network_patterns:
            if re.search(pattern, content):
                issues.append(SecurityIssue(
                    file=str(filepath),
                    line=0,
                    severity=MEDIUM,
                    category='network_access',
                    message=f'Direct network access ({pattern}) - prefer C++ layer'
                ))
        
        return issues
    
    def check_file_operations(self, filepath: Path, content: str, lines: List[str]) -> List[SecurityIssue]:
        """Check file operation patterns."""
        issues = []
        
        # Check for file operations that should be in C++
        if 'File' in content and ('read' in content.lower() or 'write' in content.lower()):
            issues.append(SecurityIssue(
                file=str(filepath),
                line=0,
                severity=MEDIUM,
                category='file_operations',
                message='File I/O in QML - consider moving to C++ layer'
            ))
        
        return issues
    
    def analyze_directory(self, directory: Path) -> List[SecurityIssue]:
        """Recursively analyze all QML files in a directory."""
        all_issues = []
        
        for root, dirs, files in os.walk(directory):
            # Skip build directories
            dirs[:] = [d for d in dirs if d not in ['build', '.git', 'node_modules']]
            
            for file in files:
                if file.endswith('.qml'):
                    filepath = Path(root) / file
                    issues = self.analyze_file(filepath)
                    all_issues.extend(issues)
        
        return all_issues
    
    def generate_report(self, issues: List[SecurityIssue]) -> str:
        """Generate a human-readable security report."""
        if not issues:
            return "✓ No security issues detected!"
        
        # Group by severity
        by_severity: Dict[str, List[SecurityIssue]] = {}
        for issue in issues:
            if issue.severity not in by_severity:
                by_severity[issue.severity] = []
            by_severity[issue.severity].append(issue)
        
        report_lines = ["QML Security Analysis Report", "=" * 50, ""]
        
        # Summary
        report_lines.append(f"Total Issues: {len(issues)}")
        for severity in [CRITICAL, HIGH, MEDIUM, LOW, INFO]:
            count = len(by_severity.get(severity, []))
            if count > 0:
                report_lines.append(f"  {severity}: {count}")
        
        report_lines.append("")
        
        # Detailed findings
        for severity in [CRITICAL, HIGH, MEDIUM, LOW, INFO]:
            severity_issues = by_severity.get(severity, [])
            if severity_issues:
                report_lines.append(f"\n{severity} Issues:")
                report_lines.append("-" * 40)
                for issue in sorted(severity_issues, key=lambda x: (x.file, x.line)):
                    report_lines.append(str(issue))
                    if issue.code_snippet:
                        report_lines.append(f"  Code: {issue.code_snippet}")
                    report_lines.append("")
        
        return '\n'.join(report_lines)


def main():
    if len(sys.argv) < 2:
        print("Usage: python qml_security_analyzer.py <qml_directory>")
        print("Example: python qml_security_analyzer.py ../qml")
        sys.exit(1)
    
    qml_dir = Path(sys.argv[1])
    
    if not qml_dir.exists():
        print(f"Error: Directory '{qml_dir}' does not exist")
        sys.exit(1)
    
    if not qml_dir.is_dir():
        print(f"Error: '{qml_dir}' is not a directory")
        sys.exit(1)
    
    analyzer = QMLSecurityAnalyzer()
    
    print(f"Analyzing QML files in {qml_dir}...")
    issues = analyzer.analyze_directory(qml_dir)
    
    report = analyzer.generate_report(issues)
    print(report)
    
    # Exit with error if critical or high severity issues found
    critical_count = sum(1 for i in issues if i.severity in [CRITICAL, HIGH])
    if critical_count > 0:
        print(f"\n⚠️  Found {critical_count} critical/high severity issues!")
        sys.exit(1)
    else:
        print("\n✓ Analysis complete - no critical issues found")
        sys.exit(0)


if __name__ == '__main__':
    main()
