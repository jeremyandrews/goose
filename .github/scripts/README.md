# GooseBot PR Review System

This directory contains scripts for Goose's automated PR review system powered by Anthropic's Claude API.

## Components

- `goosebot_review.py` - Main script for analyzing PRs and posting reviews
- `test_goosebot.py` - Testing script for local development without GitHub dependencies

## Features

GooseBot currently supports two review types:

### Clarity Review
- Analyzes PR title and description
- Suggests improvements for clarity and completeness
- Uses structured text format for responses

### Quality Review
- Analyzes code changes in PR diffs
- Evaluates against Rust best practices
- Provides categorized suggestions using JSON structure
- Focuses on error handling, performance, safety, etc.

## JSON Response Format

The quality review generates a structured JSON response:

```json
[
  {
    "category": "Error Handling",
    "description": "Issue description",
    "suggestion": "Suggested improvement",
    "impact": "High/Medium/Low - impact explanation"
  }
]
```

This structure allows for:
- Consistent formatting of suggestions
- Easy categorization of issues
- Clear impact assessments
- Potential future automation based on categories

## Usage

### GitHub Actions Workflow

The review is automatically triggered by:
- Opening a new PR
- Updating an existing PR
- Manual triggering via workflow_dispatch

### Local Testing

For local development or testing:

```bash
# Install dependencies (one-time setup)
pip install anthropic==0.45.2 PyGithub==2.6.0 python-dotenv

# Create .env file with API keys
echo "ANTHROPIC_API_KEY=your_key_here" > .env

# Run tests with mock data
python .github/scripts/test_goosebot.py --mock simple

# Test against a real PR (view-only, doesn't post comments)
python .github/scripts/test_goosebot.py --pr 123 --scope quality
```

## Configuration

The system supports various configuration options:

- File filtering via environment variables:
  - `PR_REVIEW_WHITELIST`: Comma-separated glob patterns for files to include
  - `PR_REVIEW_BLACKLIST`: Comma-separated glob patterns for files to exclude
  
- Token budget management via `TOKEN_BUDGET` environment variable
  
- Debug mode with `GOOSEBOT_DEBUG_DUMP=1` for saving prompts and responses

## Implementation Notes

- Large diffs are automatically chunked to fit within context limits
- JSON parsing includes fallback mechanisms for handling LLM formatting quirks
- Hash-based change detection prevents duplicate reviews of unchanged content
