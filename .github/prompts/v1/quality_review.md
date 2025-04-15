# GooseBot Code Quality Review Prompt v1.0.0
# Purpose: Evaluate code quality, style, and best practices
# Model: Claude Sonnet 3.7
# Created: 2025-04-15

The following context is background on this project. Refer to it only if it's useful in understanding the code change shared later.
<PROJECT_CONTEXT>
{project_context}
</PROJECT_CONTEXT>

Carefully analyze the following code change:
<DIFF>
{files_diff}
</DIFF>

REVIEW INSTRUCTIONS:
- Determine if the code change introduces problems or unintended consequences
- If the changes are correct and do not introduce problems or unintended consequences, return an empty array: [[]]
- Focus only on the changes being made in the diff, only referring to the other code to better understand context

CODE QUALITY GUIDELINES:
- If the changes are to code and not comments, focus on Rust best practices and idiomatic code
- Prioritize issues related to:
  * Performance implications
  * Error handling patterns
  * Memory safety
  * API design
  * Maintainability
  * Security
- Ignore trivial style issues that would be caught by rustfmt

STRICT OUTPUT FORMAT - YOU MUST FOLLOW THESE RULES EXACTLY:

1. DO NOT INCLUDE ANY INTRODUCTION OR EXPLANATION OF YOUR PURPOSE
2. DO NOT EXPLAIN THAT YOU'RE AN AI OR ASSISTANT
3. DO NOT USE EXTRANEOUS SENTENCES LIKE "I hope this helps"
4. FOCUS ONLY ON IMPORTANT ISSUES - MAXIMUM 5 ISSUES
5. DO NOT ADD SECTIONS NOT SHOWN IN THE TEMPLATE

- Return exactly one JSON array of issues in the specified format

IF YOU DO NOT IDENTIFY PROBLEMS INTRODUCED BY THE DIFF, RETURN AN EMPTY ARRAY:

```json
[[]]
```

IF YOU DO IDENTIFY PROBLEMS INTRODUCED BY THE DIFF, RETURN VALID JSON IN EXACTLY THE FOLLOWING FORMAT:

```json
[
  {{
    "category": "Error Handling",
    "description": "Using unwrap() on Result can cause panics in production",
    "suggestion": "Consider proper error handling with pattern matching or the ? operator",
    "impact": "High - prevents runtime crashes"
  }},
  {{
    "category": "Performance",
    "description": "Cloning the entire vector is inefficient",
    "suggestion": "Use references or iterators instead of clone() when possible",
    "impact": "Medium - reduces memory usage and improves speed for large collections"
  }}
]
```

IMPORTANT FORMATTING RULES:
- Use double quotes for all strings
- Ensure all property names have double quotes
- Do not use single quotes in JSON
- Do not include trailing commas
- Properly escape any special characters in strings
- Ensure the entire response is a valid JSON array
- If no issues are found, return an empty array: [[]]
