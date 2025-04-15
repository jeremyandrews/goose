# GooseBot - AI-Assisted Code Review Bot

GooseBot is an AI-assisted code review tool for the Goose load testing framework. It provides automated, consistent feedback on pull requests to complement human reviewers.

## Features

### Phase 1: PR Clarity Reviews
- Evaluates PR title and description clarity
- Provides conceptual suggestions that enhance understanding
- Focuses on explaining the purpose and value of changes
- Maintains concise, actionable feedback (maximum 2 issues)

### Phase 2: Code Quality Reviews
- Analyzes PR code diffs for quality and best practices
- Focuses on Rust best practices and idiomatic code
- Provides categorized feedback with impact assessment
- Handles large PRs with automatic chunking
- Returns structured, actionable suggestions

## Usage

### GitHub Workflow

GooseBot runs automatically on pull requests, or can be triggered manually:

1. **Automatic**: GooseBot will run on any PR when:
   - A new PR is opened
   - A PR is updated
   - A PR is reopened

2. **Manual**: Trigger from the Actions tab:
   - Select "GooseBot PR Review" workflow
   - Enter PR number
   - Select review scope: "clarity" or "quality"
   - Optionally check "Force review"

### Local Testing

For local development and testing without requiring a real GitHub PR:

```bash
# Install dependencies
pip install python-dotenv anthropic==0.45.2

# Test with a real PR (GitHub API)
python .github/scripts/test_goosebot.py --pr 618

# Test with a real PR using clarity scope
python .github/scripts/test_goosebot.py --pr 618 --scope clarity

# Test with mock data
python .github/scripts/test_goosebot.py --mock simple

# Test with different mock data types
python .github/scripts/test_goosebot.py --mock large
python .github/scripts/test_goosebot.py --mock error

# Test chunking functionality
python .github/scripts/test_goosebot.py --test-chunks

# Test with real API (requires .env file with ANTHROPIC_API_KEY)
python .github/scripts/test_goosebot.py --mock simple --use-real-api
python .github/scripts/test_goosebot.py --pr 618 --use-real-api

# Test with a custom diff file
python .github/scripts/test_goosebot.py --mock custom --custom-file path/to/diff.patch

# Force review of a PR even if no changes detected since last review
python .github/scripts/test_goosebot.py --pr 618 --force
```

For real API testing, create a `.env` file with:
```
ANTHROPIC_API_KEY=your_api_key_here
```

## Configuration

### File Filtering

Set environment variables in the workflow file:
```yaml
PR_REVIEW_WHITELIST: "*.rs,*.md,*.py,*.toml,*.yml,*.yaml"
PR_REVIEW_BLACKLIST: "tests/*,benches/*,target/*"
```

### Token Budget

Control API token usage with:
```yaml
TOKEN_BUDGET: "100000"
```

## Prompt Templates

Prompts are stored in `.github/prompts/{version}/{scope}_review.md`:

- `v1/clarity_review.md`: Evaluates PR title and description clarity
- `v1/quality_review.md`: Analyzes code quality and best practices

## Architecture

1. **PR Processing**:
   - Extract PR details (title, description, files changed)
   - For quality reviews, extract diff content

2. **Content Analysis**:
   - Send to Anthropic Claude API with appropriate prompt
   - For large diffs, chunk content and process each chunk

3. **Response Processing**:
   - Parse LLM response into structured format
   - Format suggestions for GitHub comment

4. **Results Posting**:
   - Post comment on PR with findings
   - Track PR hashes to avoid duplicate comments
