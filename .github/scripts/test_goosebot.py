#!/usr/bin/env python3
"""
Test script for GooseBot code quality review functionality.

This script provides functions to test the code quality review functionality
without requiring a real GitHub PR, making local testing easier.
"""

import os
import sys
import json
import argparse
import tempfile
from typing import Dict, Any, List

# Try to import dotenv, but make it optional
try:
    from dotenv import load_dotenv
except ImportError:
    # Define a mock load_dotenv function if dotenv is not available
    def load_dotenv():
        print("python-dotenv not installed, skipping .env loading")

# Add the current directory to path to import goosebot_review
sys.path.append(os.path.dirname(os.path.abspath(__file__)))
from goosebot_review import (
    FileFilterConfig, 
    TokenUsageTracker,
    chunk_content, 
    analyze_code_quality,
    format_code_suggestions,
    gather_project_context,
    call_anthropic_api
)

def create_mock_diff(mock_type: str = "simple") -> Dict[str, str]:
    """
    Create a mock diff for testing.
    
    Args:
        mock_type: Type of mock diff to create ("simple", "large", "error", etc.)
        
    Returns:
        Dictionary mapping filenames to their diff content
    """
    if mock_type == "simple":
        return {
            "src/example.rs": """
@@ -1,5 +1,6 @@
 use std::collections::HashMap;
+use std::time::Duration;
 
-fn process_data(input: &str) -> HashMap<String, u32> {
+fn process_data(input: &str) -> Result<HashMap<String, u32>, String> {
     let mut result = HashMap::new();
@@ -7,9 +8,15 @@
     for line in input.lines() {
         let parts: Vec<&str> = line.split(',').collect();
-        let key = parts[0].to_string();
-        let value = parts[1].parse::<u32>().unwrap();
-        result.insert(key, value);
+        if parts.len() < 2 {
+            return Err("Invalid input format".to_string());
+        }
+        
+        let key = parts[0].to_string();
+        match parts[1].parse::<u32>() {
+            Ok(value) => { result.insert(key, value); }
+            Err(_) => { return Err("Parse error".to_string()); }
+        }
     }
     
-    result
+    Ok(result)
 }
"""
        }
    elif mock_type == "large":
        # Create a larger diff that will need chunking
        diff = {
            "src/large_file.rs": "".join([
                f"@@ -{i},5 +{i},5 @@\n" +
                " // Original line\n" +
                "-let value = compute_value().unwrap();\n" +
                "+let value = compute_value().map_err(|e| format!(\"Error: {}\", e))?;\n" +
                " // End of hunk\n\n"
                for i in range(1, 50, 5)
            ])
        }
        return diff
    elif mock_type == "error":
        return {
            "src/error.rs": """
@@ -1,5 +1,5 @@
 pub fn unsafe_function() {
-    let ptr = std::ptr::null_mut();
-    unsafe { *ptr = 42; }  // This will cause a segfault
+    let ptr = std::ptr::null_mut::<i32>();
+    unsafe { *ptr = 42; }  // This will still cause a segfault
 }
"""
        }
    else:
        return {
            "src/test.rs": "@@ -1,1 +1,1 @@\n-// Old comment\n+// New comment\n"
        }
    
def mock_anthropic_api(prompt: str, token_tracker: TokenUsageTracker, max_tokens: int = 2000, **kwargs) -> Dict[str, Any]:
    """
    Mock the Anthropic API call for testing.
    
    Args:
        prompt: The prompt that would be sent
        token_tracker: TokenUsageTracker instance
        max_tokens: Maximum tokens to generate
        **kwargs: Other arguments (ignored in mock)
        
    Returns:
        Dictionary with the mocked API response
    """
    # Extract diff from prompt
    if "error.rs" in prompt:
        # Mock response for error file
        content = """```json
[
  {
    "category": "Safety",
    "description": "Dereferencing a null pointer causes undefined behavior and will crash the program",
    "suggestion": "// Use a safer approach like Option<T>\nlet value = None::<i32>;",
    "impact": "High - prevents program crashes and security vulnerabilities",
    "file": "src/error.rs",
    "line": 3
  }
]
```"""
    elif "large_file.rs" in prompt:
        # Mock response for large file
        content = """```json
[
  {
    "category": "Error Handling",
    "description": "Using map_err to convert error types is verbose and repetitive across the codebase",
    "suggestion": "let value = compute_value()?;",
    "impact": "Medium - improves code readability and maintainability",
    "file": "src/large_file.rs",
    "line": 5
  }
]
```"""
    else:
        # Default mock response
        content = """```json
[
  {
    "category": "Error Handling",
    "description": "Using String as an error type is limiting and doesn't provide enough context",
    "suggestion": "fn process_data(input: &str) -> Result<HashMap<String, u32>, ProcessError>",
    "impact": "Medium - improves error handling and context",
    "file": "src/example.rs",
    "line": 4
  },
  {
    "category": "Performance",
    "description": "Creating a new String for each key is inefficient",
    "suggestion": "let key = parts[0];",
    "impact": "Low - reduces memory usage slightly",
    "file": "src/example.rs",
    "line": 12
  }
]
```"""
    
    # Estimate token usage
    prompt_tokens = len(prompt.split()) * 1.3
    completion_tokens = len(content.split()) * 1.3
    
    token_tracker.record_usage(int(prompt_tokens), int(completion_tokens))
    
    return {
        "content": content,
        "prompt_tokens": int(prompt_tokens),
        "completion_tokens": int(completion_tokens)
    }

def test_quality_review(mock_type: str = "simple") -> None:
    """
    Test the code quality review functionality.
    
    Args:
        mock_type: Type of mock diff to test with
    """
    print(f"Testing quality review with {mock_type} diff...")
    
    # Create mock diff
    diff_contents = create_mock_diff(mock_type)
    
    # Initialize token tracker
    token_tracker = TokenUsageTracker(budget_limit=100000)
    
    # Create file filter
    file_filter = FileFilterConfig(whitelist_patterns="*.rs", blacklist_patterns="")
    
    # Chunk the diff content
    diff_chunks = chunk_content(diff_contents)
    print(f"Split diff into {len(diff_chunks)} chunks")
    
    # Gather project context
    project_context = gather_project_context()
    
    # Create a function to patch the API call
    original_call = call_anthropic_api
    try:
        # Patch the API call with our mock
        globals()["call_anthropic_api"] = mock_anthropic_api
        
        # Analyze code quality
        analysis_results = analyze_code_quality(diff_chunks, project_context, token_tracker)
        print(f"Generated {len(analysis_results)} analysis results")
        
        # Format suggestions - now returns both a comment and inline comments
        comment, inline_comments = format_code_suggestions(analysis_results)
        print("\n=== FORMATTED COMMENT ===\n")
        print(comment)
        print("\n=== END COMMENT ===\n")
        
        if inline_comments:
            print(f"\n=== INLINE COMMENTS ({len(inline_comments)}) ===\n")
            for i, comment in enumerate(inline_comments):
                print(f"Comment {i+1} for {comment['file']}, line {comment['line']}:")
                print(comment['body'])
                print("---")
            print("\n=== END INLINE COMMENTS ===\n")
        
        # Save to temp file for review
        with tempfile.NamedTemporaryFile(mode='w', delete=False, suffix='.md') as f:
            f.write(comment)
            print(f"Saved comment to {f.name}")
            
        # Save inline comments if any
        if inline_comments:
            with tempfile.NamedTemporaryFile(mode='w', delete=False, suffix='.json') as f:
                json.dump(inline_comments, f, indent=2)
                print(f"Saved inline comments to {f.name}")
    finally:
        # Restore original function
        globals()["call_anthropic_api"] = original_call

def test_chunk_content() -> None:
    """Test the chunk_content function with different inputs."""
    print("Testing chunk_content function...")
    
    # Simple case - small diff that fits in one chunk
    small_diff = create_mock_diff("simple")
    small_chunks = chunk_content(small_diff, max_tokens_per_chunk=10000)
    print(f"Small diff: {len(small_chunks)} chunks")
    
    # Large case - diff that needs to be split
    large_diff = create_mock_diff("large")
    large_chunks = chunk_content(large_diff, max_tokens_per_chunk=1000)
    print(f"Large diff: {len(large_chunks)} chunks")
    
    # Very small chunk size to force splitting
    tiny_chunks = chunk_content(small_diff, max_tokens_per_chunk=100)
    print(f"Tiny chunks: {len(tiny_chunks)} chunks")

def main() -> None:
    """Main function to run tests."""
    # Load environment variables from .env file if it exists
    load_dotenv()
    
    parser = argparse.ArgumentParser(description="Test GooseBot code quality review")
    
    # Create mutually exclusive group for PR vs mock testing
    mode_group = parser.add_mutually_exclusive_group(required=True)
    mode_group.add_argument("--pr", type=int, help="PR number to review (uses real GitHub API)")
    mode_group.add_argument("--mock", choices=["simple", "large", "error", "custom"], 
                            help="Use mock data for testing (specify mock type)")
    
    # Other arguments
    parser.add_argument("--scope", choices=["clarity", "quality"], default="quality",
                        help="Review scope (only applicable with --pr)")
    parser.add_argument("--test-chunks", action="store_true", help="Test chunk_content function")
    parser.add_argument("--use-real-api", action="store_true", 
                        help="Use real Anthropic API instead of mock (applies to both PR and mock modes)")
    parser.add_argument("--custom-file", type=str, help="Path to custom diff file for mock testing")
    parser.add_argument("--force", action="store_true", 
                        help="Force review even if no changes detected (only applicable with --pr)")
    parser.add_argument("--debug", action="store_true", help="Enable debug logging")
    
    args = parser.parse_args()
    
    if args.test_chunks:
        test_chunk_content()
        return
    
    if args.pr:
        # Run actual GooseBot review
        from goosebot_review import main as goosebot_main
        
        # Prepare arguments
        sys.argv = [sys.argv[0], 
                    "--pr", str(args.pr), 
                    "--scope", args.scope, 
                    "--debug"]
        
        # Add force flag if specified
        if args.force:
            sys.argv.append("--force")
            
        # Run the main GooseBot review process
        print(f"Running real GooseBot review for PR #{args.pr} with scope: {args.scope}")
        
        # Monkey patch the post_pr_comment function to avoid GitHub API calls in test mode
        from goosebot_review import post_pr_comment as original_post_pr_comment
        
        def test_post_pr_comment(pr, comment_text, pr_hash=None):
            print("\n===== COMMENT THAT WOULD BE POSTED =====")
            print(comment_text)
            print("===== END COMMENT =====\n")
            print("GitHub API call skipped in test mode")
            return True
        
        # Replace the function with our test version
        import goosebot_review
        goosebot_review.post_pr_comment = test_post_pr_comment
        
        # Run the main function
        goosebot_main()
        return
    
    # Handle mock mode
    # If using custom file, read it
    if args.mock == "custom" and args.custom_file:
        with open(args.custom_file, 'r') as f:
            custom_diff = {os.path.basename(args.custom_file): f.read()}
        diff_contents = custom_diff
    else:
        diff_contents = create_mock_diff(args.mock)
    
    # Use real API if requested
    if args.use_real_api:
        print("Using real Anthropic API (ensure ANTHROPIC_API_KEY is set in .env file)")
    else:
        # Patch the API call with our mock
        original_call = call_anthropic_api
        globals()["call_anthropic_api"] = mock_anthropic_api
    
    try:
        # Initialize token tracker
        token_tracker = TokenUsageTracker(budget_limit=100000)
        
        # Chunk the diff content
        diff_chunks = chunk_content(diff_contents)
        print(f"Split diff into {len(diff_chunks)} chunks")
        
        # Gather project context
        project_context = gather_project_context()
        
        # Analyze code quality
        analysis_results = analyze_code_quality(diff_chunks, project_context, token_tracker)
        print(f"Generated {len(analysis_results)} analysis results")
        
        # Format suggestions - now returns both a comment and inline comments
        comment, inline_comments = format_code_suggestions(analysis_results)
        print("\n=== FORMATTED COMMENT ===\n")
        print(comment)
        print("\n=== END COMMENT ===\n")
        
        if inline_comments:
            print(f"\n=== INLINE COMMENTS ({len(inline_comments)}) ===\n")
            for i, comment in enumerate(inline_comments):
                print(f"Comment {i+1} for {comment['file']}, line {comment['line']}:")
                print(comment['body'])
                print("---")
            print("\n=== END INLINE COMMENTS ===\n")
        
        # Save to temp file for review
        with tempfile.NamedTemporaryFile(mode='w', delete=False, suffix='.md') as f:
            f.write(comment)
            print(f"Saved comment to {f.name}")
            
        # Save inline comments if any
        if inline_comments:
            with tempfile.NamedTemporaryFile(mode='w', delete=False, suffix='.json') as f:
                json.dump(inline_comments, f, indent=2)
                print(f"Saved inline comments to {f.name}")
    finally:
        # Restore original function if patched
        if not args.use_real_api:
            globals()["call_anthropic_api"] = original_call

if __name__ == "__main__":
    main()
