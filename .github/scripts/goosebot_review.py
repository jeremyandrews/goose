#!/usr/bin/env python3
"""
GooseBot - AI Code Review Bot for Goose Load Testing Framework

This script fetches PR details, gathers project context from memory-bank,
sends this information to the Anthropic API, and posts the review as a comment.
"""

import os
import sys
import argparse
import logging
import json
import re
import hashlib
from github import Github
from github.PullRequest import PullRequest
import anthropic
import fnmatch
import base64
from typing import List, Dict, Any, Optional, Tuple

# Configure logging
logging.basicConfig(
    level=logging.INFO,
    format="%(asctime)s - %(name)s - %(levelname)s - %(message)s",
)
logger = logging.getLogger("goosebot")

class FileFilterConfig:
    """Configuration for filtering files that should be reviewed."""
    
    def __init__(self, whitelist_patterns: str = "*", blacklist_patterns: str = ""):
        """
        Initialize file filter configuration.
        
        Args:
            whitelist_patterns: Comma-separated glob patterns for files to include
            blacklist_patterns: Comma-separated glob patterns for files to exclude
        """
        self.whitelist_patterns = whitelist_patterns.split(",") if whitelist_patterns else ["*"]
        self.blacklist_patterns = blacklist_patterns.split(",") if blacklist_patterns else []
        
    @classmethod
    def from_env(cls) -> 'FileFilterConfig':
        """Create a FileFilterConfig from environment variables."""
        return cls(
            whitelist_patterns=os.environ.get("PR_REVIEW_WHITELIST", "*"),
            blacklist_patterns=os.environ.get("PR_REVIEW_BLACKLIST", "")
        )
        
    def should_review_file(self, filename: str) -> bool:
        """
        Determine if a file should be reviewed based on patterns.
        
        Args:
            filename: The name of the file to check
            
        Returns:
            True if the file should be reviewed, False otherwise
        """
        # Check blacklist first (takes precedence)
        for pattern in self.blacklist_patterns:
            if fnmatch.fnmatch(filename, pattern):
                logger.debug(f"Skipping {filename} (matches blacklist pattern {pattern})")
                return False
                
        # Then check if matches any whitelist pattern
        for pattern in self.whitelist_patterns:
            if fnmatch.fnmatch(filename, pattern):
                logger.debug(f"Including {filename} (matches whitelist pattern {pattern})")
                return True
                
        # If no whitelist patterns match, exclude the file
        logger.debug(f"Skipping {filename} (doesn't match any whitelist pattern)")
        return False

class TokenUsageTracker:
    """Track token usage for Anthropic API calls."""
    
    def __init__(self, budget_limit: int = 100000):
        """
        Initialize token usage tracking.
        
        Args:
            budget_limit: Maximum tokens to use across all API calls
        """
        self.budget_limit = budget_limit
        self.current_usage = 0
        
    def can_process(self, estimated_tokens: int) -> bool:
        """
        Check if there's enough budget for the estimated token usage.
        
        Args:
            estimated_tokens: Estimated tokens for the next API call
            
        Returns:
            True if the estimated usage is within budget, False otherwise
        """
        return self.current_usage + estimated_tokens <= self.budget_limit
        
    def record_usage(self, prompt_tokens: int, completion_tokens: int) -> int:
        """
        Record tokens used from an API call.
        
        Args:
            prompt_tokens: Number of tokens in the prompt
            completion_tokens: Number of tokens in the completion
            
        Returns:
            Updated total token usage
        """
        usage = prompt_tokens + completion_tokens
        self.current_usage += usage
        logger.info(f"API call used {prompt_tokens} prompt tokens + {completion_tokens} completion tokens = {usage} total")
        logger.info(f"Total usage: {self.current_usage}/{self.budget_limit} tokens ({(self.current_usage/self.budget_limit)*100:.1f}%)")
        return self.current_usage

def get_pr_diff(pr: PullRequest, file_filter: FileFilterConfig) -> Dict[str, str]:
    """
    Extract the diff content for a PR, filtered by relevant files.
    
    Args:
        pr: GitHub PullRequest object
        file_filter: FileFilterConfig object for filtering relevant files
        
    Returns:
        Dictionary mapping filenames to their diff content
    """
    diff_contents = {}
    
    for file in pr.get_files():
        # Skip files we don't want to review
        if not file_filter.should_review_file(file.filename):
            logger.debug(f"Skipping diff for {file.filename} (filtered out)")
            continue
            
        # Skip files with no content changes (e.g., binary files, renames)
        if file.patch is None:
            logger.debug(f"Skipping {file.filename} (no diff content available)")
            continue
            
        logger.info(f"Processing diff for {file.filename} (+{file.additions}, -{file.deletions})")
        diff_contents[file.filename] = file.patch
        
    return diff_contents

def chunk_content(content: Dict[str, str], max_tokens_per_chunk: int = 5000) -> List[Dict[str, Any]]:
    """
    Break large diffs into manageable chunks while preserving context.
    
    Args:
        content: Dictionary mapping filenames to diff content
        max_tokens_per_chunk: Maximum tokens per chunk
        
    Returns:
        List of chunks, each a dict with files and their partial diffs
    """
    chunks = []
    current_chunk = {}
    current_tokens = 0
    
    for filename, diff in content.items():
        # Simple token estimation (can be improved)
        estimated_tokens = len(diff.split()) * 1.3
        
        # If this file would exceed chunk size, create a new chunk
        if current_tokens > 0 and current_tokens + estimated_tokens > max_tokens_per_chunk:
            chunks.append({
                'files': current_chunk,
                'estimated_tokens': current_tokens
            })
            current_chunk = {}
            current_tokens = 0
        
        # If a single file is too large, split it (simplistic approach)
        if estimated_tokens > max_tokens_per_chunk:
            logger.warning(f"File {filename} is very large, splitting into multiple chunks")
            
            # Split by hunks (sections starting with @@ which indicate line ranges)
            hunks = re.split(r'(^@@.*?@@.*?$)', diff, flags=re.MULTILINE)
            
            # Group hunks into reasonable sizes
            hunk_chunks = []
            current_hunk_chunk = []
            current_hunk_tokens = 0
            
            # Ensure headers stay with their content
            for i in range(0, len(hunks), 2):
                if i+1 < len(hunks):  # Check if we have both header and content
                    hunk_header = hunks[i]
                    hunk_content = hunks[i+1]
                    hunk_tokens = (len(hunk_header.split()) + len(hunk_content.split())) * 1.3
                    
                    if current_hunk_tokens + hunk_tokens > max_tokens_per_chunk:
                        if current_hunk_chunk:
                            hunk_chunks.append("".join(current_hunk_chunk))
                            current_hunk_chunk = []
                            current_hunk_tokens = 0
                            
                    current_hunk_chunk.extend([hunk_header, hunk_content])
                    current_hunk_tokens += hunk_tokens
            
            # Add any remaining hunks
            if current_hunk_chunk:
                hunk_chunks.append("".join(current_hunk_chunk))
            
            # Process each hunk chunk as a separate file entry
            for i, hunk_chunk in enumerate(hunk_chunks):
                chunk_filename = f"{filename} (part {i+1}/{len(hunk_chunks)})"
                current_chunk[chunk_filename] = hunk_chunk
                current_tokens += len(hunk_chunk.split()) * 1.3
                
                # If this chunk is full, start a new one
                if current_tokens > max_tokens_per_chunk:
                    chunks.append({
                        'files': current_chunk,
                        'estimated_tokens': current_tokens
                    })
                    current_chunk = {}
                    current_tokens = 0
        else:
            # Regular case: file fits in a chunk
            current_chunk[filename] = diff
            current_tokens += estimated_tokens
    
    # Add the last chunk if it has content
    if current_chunk:
        chunks.append({
            'files': current_chunk,
            'estimated_tokens': current_tokens
        })
    
    logger.info(f"Split diff into {len(chunks)} chunks for processing")
    return chunks

def analyze_code_quality(diff_chunks: List[Dict[str, Any]], project_context: str, token_tracker: TokenUsageTracker) -> List[Dict[str, Any]]:
    """
    Analyze code quality by sending diff chunks to the LLM.
    
    Args:
        diff_chunks: List of chunks containing file diffs
        project_context: String containing project context
        token_tracker: TokenUsageTracker for monitoring usage
        
    Returns:
        List of dictionaries containing analysis results for each chunk
    """
    results = []
    
    # Debug option to dump files
    debug_dump = os.environ.get("GOOSEBOT_DEBUG_DUMP") == "1"
    
    # Load quality review prompt template
    prompt_template = load_prompt_template("quality", "v1")
    
    for i, chunk in enumerate(diff_chunks):
        logger.info(f"Analyzing chunk {i+1}/{len(diff_chunks)} (~{int(chunk['estimated_tokens'])} tokens)")
        
        try:
            # Format diff content for the prompt
            files_diff = ""
            for filename, diff in chunk['files'].items():
                files_diff += f"### {filename}\n```diff\n{diff}\n```\n\n"
            
            # Create the prompt
            prompt = prompt_template.format(
                project_context=project_context,
                files_diff=files_diff
            )
            
            # Add complete prompt logging
            print("\n===== COMPLETE PROMPT SENT TO LLM =====")
            print(prompt)  # Print entire prompt, no truncation
            print("===== END PROMPT =====\n")

            # Optional file dump for future reference
            if debug_dump:
                dump_path = os.path.join(os.getcwd(), f"goosebot_prompt_{i}.txt")
                try:
                    with open(dump_path, "w") as f:
                        f.write(prompt)
                    print(f"Prompt also saved to {dump_path}")
                except Exception as e:
                    print(f"Failed to write prompt dump: {e}")
            
            # Call Anthropic API
            response = call_anthropic_api(prompt, token_tracker, max_tokens=2000)
            
            # DEBUG - Print entire raw response
            print("\n=== COMPLETE RAW API RESPONSE ===")
            print(response["content"])
            print("=== END RAW RESPONSE ===\n")
            
            # Store results
            results.append({
                'chunk_index': i,
                'files': list(chunk['files'].keys()),
                'analysis': response["content"]
            })
        except Exception as e:
            print(f"CRITICAL ERROR in analyze_code_quality for chunk {i+1}: {e}")
            import traceback
            traceback.print_exc()
            
            # Add an error result
            results.append({
                'chunk_index': i,
                'files': list(chunk['files'].keys()),
                'analysis': f"Error during analysis: {str(e)}"
            })
    
    return results

def format_code_suggestions(analysis_results: List[Dict[str, Any]]) -> str:
    """
    Format code suggestions into a well-structured comment.
    
    Args:
        analysis_results: Results from analyze_code_quality()
        
    Returns:
        Formatted comment string
    """
    # Check if we have any results
    if not analysis_results:
        return "### GooseBot Code Quality Review\n\nNo issues found in the code."
    
    comment = "### GooseBot Code Quality Review\n\n"
    
    # Debug option to dump raw responses
    debug_dump = os.environ.get("GOOSEBOT_DEBUG_DUMP") == "1"
    
    # Extract suggestions from all chunks
    all_suggestions = []
    for i, result in enumerate(analysis_results):
        # Always print raw response for debugging
        print("\n==== RAW LLM RESPONSE ====")
        print(result['analysis'])
        print("==== END RAW RESPONSE ====\n")
        
        # Dump raw response to file if enabled
        if debug_dump:
            dump_path = os.path.join(os.getcwd(), f"goosebot_response_{i}.txt")
            try:
                with open(dump_path, "w") as f:
                    f.write(result['analysis'])
                print(f"Dumped raw response to {dump_path}")
            except Exception as e:
                print(f"Failed to write debug dump: {e}")
        
        try:
            # Try to parse JSON response if the LLM followed the structured format
            match = re.search(r'```json\s*(.*?)\s*```', result['analysis'], re.DOTALL)
            if match:
                # Extract and clean up the JSON string
                json_str = match.group(1).strip()
                
                # Handle special case for empty array (typo fix PRs)
                if json_str.strip() == "[[]]":
                    logger.info("Found empty array response [[]] - typo fix PR detected")
                    return "### GooseBot Code Quality Review\n\nNo issues found. Typo fixes look good!"
                
                # Handle the specific error we're seeing with newlines and formatting
                if json_str.startswith('\n'):
                    print("WARNING: JSON starts with newline, fixing...")
                    json_str = json_str.lstrip()
                
                # Special case for the specific error we're seeing
                if '\n    "category"' in json_str:
                    print("FIXING SPECIFIC ERROR: Found '\\n    \"category\"'")
                    # This might be a JSON array that's not properly formatted
                    # Try to repair it by adding square brackets and commas
                    if not json_str.startswith('['):
                        json_str = '[' + json_str
                    if not json_str.endswith(']'):
                        json_str = json_str + ']'
                    # Replace newline+indent before "category" with comma
                    json_str = re.sub(r'\n\s*"category"', ',"category"', json_str)
                
                # Log the JSON content for debugging
                print(f"JSON STRING (after fixes): {json_str[:200]}...")
                logger.debug(f"Extracted JSON: {json_str[:100]}...")
                
                try:
                    # Attempt to parse the JSON
                    suggestions = json.loads(json_str)
                    all_suggestions.extend(suggestions)
                except json.JSONDecodeError as e:
                    # Better JSON error reporting
                    position = e.pos
                    context_start = max(0, position - 20)
                    context_end = min(len(json_str), position + 20)
                    error_context = json_str[context_start:context_end]
                    
                    logger.error(f"JSON parse error at position {position}: {e.msg}")
                    logger.error(f"Context around error: '...{error_context}...'")
                    
                    # Simple fallback: try to extract individual JSON objects
                    logger.info("Attempting fallback JSON parsing...")
                    try:
                        # Try to extract and parse individual JSON objects
                        objects_pattern = r'\{\s*"category"\s*:.*?\}\s*(?=,|\]|$)'
                        matches = re.findall(objects_pattern, json_str, re.DOTALL)
                        
                        if matches:
                            logger.info(f"Found {len(matches)} potential JSON objects using fallback method")
                            for obj_str in matches:
                                try:
                                    # Ensure it's a valid object with surrounding braces
                                    if not obj_str.strip().startswith("{"):
                                        obj_str = "{" + obj_str
                                    if not obj_str.strip().endswith("}"):
                                        obj_str = obj_str + "}"
                                    
                                    # Try to parse the individual object
                                    suggestion = json.loads(obj_str)
                                    all_suggestions.append(suggestion)
                                    logger.info("Successfully parsed suggestion with fallback")
                                except json.JSONDecodeError:
                                    # Skip this object if it can't be parsed
                                    continue
                        
                        # If fallback didn't work, add a generic suggestion with raw content
                        if not matches:
                            raise Exception("Fallback parsing failed")
                    except Exception as fallback_error:
                        logger.error(f"Fallback parsing failed: {fallback_error}")
                        all_suggestions.append({
                            'category': 'Parsing Error',
                            'description': f"Could not parse structured response: {e}",
                            'suggestion': "Review the raw LLM output:\n\n```\n" + json_str[:500] + "...\n```",
                            'impact': 'Unknown'
                        })
            else:
                # Fallback to extracting suggestions from text format
                logger.warning(f"Unable to parse JSON from LLM response, using fallback extraction")
                # Just add the raw analysis as a single suggestion
                all_suggestions.append({
                    'category': 'General Feedback',
                    'description': result['analysis'],
                    'suggestion': 'See description for details.',
                    'impact': 'Unknown'
                })
        except Exception as e:
            logger.error(f"Error parsing suggestions: {e}")
            # Include raw response if parsing fails
            all_suggestions.append({
                'category': 'Parsing Error',
                'description': f"Error parsing suggestions: {e}",
                'suggestion': result['analysis'][:1000] + ("..." if len(result['analysis']) > 1000 else ""),
                'impact': 'Unknown'
            })
    
    # Limit to top 5 issues (if more than 5)
    if len(all_suggestions) > 5:
        logger.info(f"Limiting from {len(all_suggestions)} to top 5 issues")
        all_suggestions = all_suggestions[:5]
    
    # Group suggestions by category
    categories = {}
    for suggestion in all_suggestions:
        category = suggestion.get('category', 'General')
        if category not in categories:
            categories[category] = []
        categories[category].append(suggestion)
    
    # Build comment with categorized suggestions
    for category, suggestions in categories.items():
        comment += f"#### {category}\n\n"
        for suggestion in suggestions:
            comment += f"**Issue:** {suggestion.get('description', 'No description provided')}\n\n"
            comment += f"**Suggestion:** {suggestion.get('suggestion', 'No suggestion provided')}\n\n"
            if 'impact' in suggestion:
                comment += f"**Impact:** {suggestion['impact']}\n\n"
            comment += "---\n\n"
    
    return comment

def gather_project_context() -> str:
    """
    Read memory-bank files to provide context to the AI.
    
    Returns:
        String containing content from memory-bank files
    """
    context = ""
    
    # Priority order for files
    context_files = [
        "projectbrief.md",
        "productContext.md", 
        "systemPatterns.md",
        "techContext.md",
        "activeContext.md",
        "progress.md"
    ]
    
    for filename in context_files:
        path = f"memory-bank/{filename}"
        if os.path.exists(path):
            logger.info(f"Loading context from {path}")
            with open(path, 'r') as f:
                content = f.read()
                context += f"## {filename}\n{content}\n\n"
        else:
            logger.warning(f"Context file {path} not found")
    
    if not context:
        logger.warning("No context files found in memory-bank/")
        
    return context

def load_prompt_template(scope: str, version: str = "v1") -> str:
    """
    Load the prompt template for the specified scope and version.
    
    Args:
        scope: The review scope (e.g., 'clarity')
        version: The prompt version to use
        
    Returns:
        The prompt template as a string
    """
    template_path = f".github/prompts/{version}/{scope}_review.md"
    
    try:
        with open(template_path, 'r') as f:
            return f.read()
    except FileNotFoundError:
        logger.error(f"Prompt template not found: {template_path}")
        sys.exit(1)

def get_pull_request(repo, pr_number: int) -> PullRequest:
    """
    Get the pull request object from GitHub.
    
    Args:
        repo: GitHub repository object
        pr_number: Pull request number
        
    Returns:
        GitHub PullRequest object
    """
    try:
        return repo.get_pull(pr_number)
    except Exception as e:
        logger.error(f"Failed to get PR #{pr_number}: {e}")
        sys.exit(1)

def get_pr_details(pr: PullRequest) -> Dict[str, Any]:
    """
    Extract relevant details from a pull request.
    
    Args:
        pr: GitHub PullRequest object
        
    Returns:
        Dictionary containing PR title, description, and files changed
    """
    logger.info(f"Getting details for PR #{pr.number}: {pr.title}")
    
    files_changed = []
    try:
        for file in pr.get_files():
            files_changed.append({
                'filename': file.filename,
                'status': file.status,  # added, modified, removed
                'additions': file.additions,
                'deletions': file.deletions,
                'changes': file.changes
            })
    except Exception as e:
        logger.error(f"Failed to get files changed: {e}")
    
    return {
        'title': pr.title,
        'description': pr.body or "",
        'files_changed': files_changed,
        'author': pr.user.login if pr.user else "Unknown"
    }

def filter_relevant_files(files_changed: List[Dict[str, Any]], file_filter: FileFilterConfig) -> List[Dict[str, Any]]:
    """
    Filter files based on filter configuration.
    
    Args:
        files_changed: List of file change dictionaries
        file_filter: FileFilterConfig object
        
    Returns:
        Filtered list of file change dictionaries
    """
    relevant_files = []
    
    for file in files_changed:
        if file_filter.should_review_file(file['filename']):
            relevant_files.append(file)
    
    logger.info(f"Filtered {len(files_changed)} files to {len(relevant_files)} relevant files")
    return relevant_files

def get_file_content(repo, file_path: str, ref: str = "main") -> Optional[str]:
    """
    Get the content of a file from the repository.
    
    Args:
        repo: GitHub repository object
        file_path: Path to the file
        ref: Branch or commit reference
        
    Returns:
        File content as string, or None if not found
    """
    try:
        content_file = repo.get_contents(file_path, ref=ref)
        content = base64.b64decode(content_file.content).decode('utf-8')
        return content
    except Exception as e:
        logger.warning(f"Failed to get content for {file_path}: {e}")
        return None

def call_anthropic_api(prompt: str, token_tracker: TokenUsageTracker, max_tokens: int = 4000) -> Dict[str, Any]:
    """
    Send prompt to Anthropic API and get response.
    
    Args:
        prompt: The prompt to send
        token_tracker: TokenUsageTracker to record usage
        max_tokens: Maximum tokens to generate
        
    Returns:
        Dictionary with the API response and token usage
    """
    # Simple environment variable debugging
    logger.info("Checking API environment variables:")
    logger.info(f"ANTHROPIC_API_KEY present in environment: {'ANTHROPIC_API_KEY' in os.environ}")
    logger.info(f"ANTHROPIC_API_URL present in environment: {'ANTHROPIC_API_URL' in os.environ}")
    
    # Check if key has content without revealing it
    debug_key = os.environ.get("ANTHROPIC_API_KEY", "")
    logger.info(f"ANTHROPIC_API_KEY length: {len(debug_key)}")
    logger.info(f"ANTHROPIC_API_KEY first 4 chars: {debug_key[:4] if len(debug_key) > 4 else 'empty'}")
    
    api_key = os.environ.get("ANTHROPIC_API_KEY")
    # Check if the key exists AND has a non-empty value
    if api_key is None or api_key.strip() == "":
        logger.error("ANTHROPIC_API_KEY environment variable not set or is empty")
        logger.info("Please ensure the secret is set correctly in GitHub repository settings")
        logger.info("See: https://github.com/tag1consulting/goose/settings/secrets/actions")
        sys.exit(1)
    
    # Get custom API URL if set
    api_url = os.environ.get("ANTHROPIC_API_URL")
    
    # Estimate token count (rough approximation)
    estimated_tokens = len(prompt.split()) * 1.3
    
    if not token_tracker.can_process(estimated_tokens + max_tokens):
        logger.error(f"Token budget exceeded. Estimated prompt: {estimated_tokens}, response: {max_tokens}")
        return {
            "content": "Error: Token budget exceeded. Unable to complete review.",
            "prompt_tokens": 0,
            "completion_tokens": 0
        }
    
    try:
        logger.info("Calling Anthropic API...")
        
        # Configure client with custom URL if provided
        client_kwargs = {"api_key": api_key}
        if api_url:
            logger.info(f"Using custom API URL: {api_url}")
            client_kwargs["base_url"] = api_url
            
        client = anthropic.Anthropic(**client_kwargs)
        
        # Use Claude Sonnet 3.7 model
        response = client.messages.create(
            model="claude-3-sonnet-20240229",
            max_tokens=max_tokens,
            system="You are GooseBot, an AI assistant that helps with code reviews for the Goose load testing framework. Be concise and helpful.",
            messages=[
                {"role": "user", "content": prompt}
            ]
        )
        
        prompt_tokens = response.usage.input_tokens
        completion_tokens = response.usage.output_tokens
        
        token_tracker.record_usage(prompt_tokens, completion_tokens)
        
        return {
            "content": response.content[0].text,
            "prompt_tokens": prompt_tokens,
            "completion_tokens": completion_tokens
        }
    except Exception as e:
        logger.error(f"Error calling Anthropic API: {e}")
        return {
            "content": f"Error: Failed to get response from Anthropic API: {e}",
            "prompt_tokens": 0,
            "completion_tokens": 0
        }

def format_files_changed_summary(files: List[Dict[str, Any]]) -> str:
    """
    Format files changed for the prompt.
    
    Args:
        files: List of file change dictionaries
        
    Returns:
        Formatted string of files changed
    """
    result = ""
    for file in files:
        result += f"- {file['filename']} ({file['status']}, +{file['additions']}, -{file['deletions']})\n"
    return result

def analyze_previous_reviews(pr: PullRequest) -> Dict[str, Any]:
    """
    Analyze previous GooseBot comments to extract:
    1. The PR hash that was reviewed
    2. The suggestions that were made
    
    Args:
        pr: GitHub PullRequest object
        
    Returns:
        dict: {
            'last_hash': hash string or None,
            'previous_suggestions': list of suggestion texts or [],
            'last_comment_id': ID of last comment for potential updating
        }
    """
    comments = pr.get_issue_comments()
    result = {
        'last_hash': None,
        'previous_suggestions': [],
        'last_comment_id': None
    }
    
    for comment in comments:
        if comment.user.login == "github-actions[bot]" and "### GooseBot" in comment.body:
            # Extract hash
            hash_match = re.search(r'<!-- PR-HASH: ([a-f0-9]+) -->', comment.body)
            if hash_match:
                result['last_hash'] = hash_match.group(1)
            
            # Extract suggestions
            if "Title suggestion:" in comment.body:
                title_match = re.search(r'Title suggestion: (.*?)(?=\n\n|$)', comment.body, re.DOTALL)
                if title_match:
                    result['previous_suggestions'].append(title_match.group(1).strip())
            
            if "Description enhancement:" in comment.body:
                desc_match = re.search(r'Description enhancement: (.*?)(?=\n\n|$)', comment.body, re.DOTALL)
                if desc_match:
                    result['previous_suggestions'].append(desc_match.group(1).strip())
            
            # Keep track of most recent comment ID
            result['last_comment_id'] = comment.id
    
    return result

def extract_suggestions_from_response(response_text: str) -> List[str]:
    """
    Extract suggestions from the LLM response.
    
    Args:
        response_text: Text response from the LLM
        
    Returns:
        List of suggestion strings
    """
    suggestions = []
    
    if "Title suggestion:" in response_text:
        title_match = re.search(r'Title suggestion: (.*?)(?=\n\n|$)', response_text, re.DOTALL)
        if title_match:
            suggestions.append(title_match.group(1).strip())
    
    if "Description enhancement:" in response_text:
        desc_match = re.search(r'Description enhancement: (.*?)(?=\n\n|$)', response_text, re.DOTALL)
        if desc_match:
            suggestions.append(desc_match.group(1).strip())
    
    return suggestions

def needs_new_review(pr: PullRequest, new_suggestions: List[str], previous_analysis: Dict[str, Any]) -> Tuple[bool, str]:
    """
    Determine if we need to post a new review by:
    1. Checking if PR metadata (title/description) has changed
    2. Checking if our suggestions would be different from previous ones
    
    Args:
        pr: The PR object
        new_suggestions: List of new suggestions we would make
        previous_analysis: Result from analyze_previous_reviews()
        
    Returns:
        Tuple: (needs_review, current_hash)
    """
    # Calculate current hash
    current_hash = hashlib.md5(f"{pr.title}|{pr.body or ''}".encode()).hexdigest()
    
    # Check if PR metadata changed
    pr_changed = previous_analysis['last_hash'] is None or previous_analysis['last_hash'] != current_hash
    
    # Check if suggestions changed
    previous_suggestions = previous_analysis['previous_suggestions']
    suggestions_changed = set(new_suggestions) != set(previous_suggestions)
    
    # Need review if either PR or our suggestions changed
    needs_review = pr_changed or suggestions_changed
    
    if not pr_changed:
        logger.info("PR title and description unchanged since last review.")
    
    if not suggestions_changed and previous_suggestions:
        logger.info("Our suggestions would be the same as before.")
    
    return needs_review, current_hash

def post_pr_comment(pr: PullRequest, comment_text: str, pr_hash: str = None) -> bool:
    """
    Post a comment on a pull request.
    
    Args:
        pr: GitHub PullRequest object
        comment_text: Comment text to post
        pr_hash: Hash of PR title and description to include in hidden comment
        
    Returns:
        True if successful, False otherwise
    """
    try:
        # Add hash metadata if provided
        if pr_hash:
            comment_with_hash = f"{comment_text}\n<!-- PR-HASH: {pr_hash} -->"
        else:
            comment_with_hash = comment_text
            
        pr.create_issue_comment(comment_with_hash)
        logger.info(f"Posted comment on PR #{pr.number}")
        return True
    except Exception as e:
        logger.error(f"Failed to post comment: {e}")
        return False

def main():
    """Main function to run the GooseBot review process."""
    parser = argparse.ArgumentParser(description="GooseBot PR Review")
    parser.add_argument("--pr", type=int, required=True, help="PR number to review")
    parser.add_argument("--scope", type=str, default="clarity", help="Review scope (e.g., clarity)")
    parser.add_argument("--debug", action="store_true", help="Enable debug logging")
    parser.add_argument("--version", type=str, default="v1", help="Prompt version to use")
    parser.add_argument("--force", action="store_true", help="Force review even if no changes detected")
    args = parser.parse_args()
    
    if args.debug:
        logger.setLevel(logging.DEBUG)
        
    logger.info(f"Starting GooseBot review for PR #{args.pr} (scope: {args.scope}, version: {args.version})")
    
    # Initialize token tracker
    token_tracker = TokenUsageTracker(budget_limit=int(os.environ.get("TOKEN_BUDGET", "100000")))
    
    # Initialize file filter from environment
    file_filter = FileFilterConfig.from_env()
    
    # Initialize GitHub API client
    github_token = os.environ.get("GITHUB_TOKEN")
    if not github_token:
        logger.error("GITHUB_TOKEN environment variable not set")
        sys.exit(1)
        
    try:
        # Get repository
        repo_name = os.environ.get("GITHUB_REPOSITORY", "tag1consulting/goose")
        g = Github(github_token)
        repo = g.get_repo(repo_name)
        logger.info(f"Connected to GitHub repository: {repo.full_name}")
        
        # Get PR details
        pr = get_pull_request(repo, args.pr)
        pr_details = get_pr_details(pr)
        
        # Analyze previous reviews to see if we've already commented
        previous_analysis = analyze_previous_reviews(pr)
        
        # Calculate current PR hash
        current_hash = hashlib.md5(f"{pr.title}|{pr.body or ''}".encode()).hexdigest()
        
        # If PR title/description hasn't changed and we're not forcing a review, skip
        if previous_analysis['last_hash'] == current_hash and not args.force:
            logger.info("PR title and description unchanged since last review. Skipping.")
            return
        
        # Filter relevant files
        relevant_files = filter_relevant_files(pr_details['files_changed'], file_filter)
        
        if not relevant_files:
            logger.warning("No relevant files found to review")
            post_pr_comment(pr, "## GooseBot PR Review\n\nNo relevant files found to review based on current filter settings.")
            return
            
        # Generate files changed summary
        files_changed_summary = format_files_changed_summary(relevant_files)
        
        # Gather project context from memory-bank
        project_context = gather_project_context()
        
        # Load prompt template
        prompt_template = load_prompt_template(args.scope, args.version)
        
        # Handle different review scopes
        if args.scope == "quality":
            logger.info("Performing code quality review")
            
            # Get PR diff
            diff_contents = get_pr_diff(pr, file_filter)
            
            if not diff_contents:
                logger.warning("No relevant diff content found to review")
                post_pr_comment(pr, "### GooseBot Code Quality Review\n\nNo relevant diff content found to review based on current filter settings.")
                return
                
            # Chunk the diff content
            diff_chunks = chunk_content(diff_contents)
            
            # Analyze code quality
            analysis_results = analyze_code_quality(diff_chunks, project_context, token_tracker)
            
            # Format suggestions
            comment_text = format_code_suggestions(analysis_results)
            
            # Post comment
            logger.info("Posting code quality review comment")
            post_pr_comment(pr, comment_text, current_hash)
        else:
            # Original clarity review code
            logger.info("Performing clarity review")
            
            # Format the prompt
            prompt = prompt_template.format(
                project_context=project_context,
                pr_title=pr_details['title'],
                pr_description=pr_details['description'],
                files_changed=files_changed_summary
            )
            
            # Call Anthropic API
            response = call_anthropic_api(prompt, token_tracker)
            
            if "error" in response["content"].lower():
                logger.error(f"API returned an error: {response['content']}")
                post_pr_comment(pr, f"## GooseBot Error\n\n{response['content']}")
                return
                
            # Extract suggestions from the response
            new_suggestions = extract_suggestions_from_response(response["content"])
            
            # Check if we need to post a new comment based on content changes
            if not args.force:
                # Compare to previous suggestions
                previous_suggestions = previous_analysis['previous_suggestions']
                if set(new_suggestions) == set(previous_suggestions) and previous_suggestions:
                    logger.info("Our suggestions would be the same as before. Skipping comment.")
                    return
            
            # If we get here, we need to post a new comment
            logger.info("Posting clarity review comment")
            post_pr_comment(pr, response["content"], current_hash)
        
    except Exception as e:
        logger.error(f"Unexpected error: {e}")
        sys.exit(1)

if __name__ == "__main__":
    main()
