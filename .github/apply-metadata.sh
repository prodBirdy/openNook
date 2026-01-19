#!/usr/bin/env bash
# Script to apply repository metadata (description and topics)
# Requires: GitHub CLI (gh) to be installed and authenticated

set -e

REPO="prodBirdy/openNook"
DESCRIPTION="An open-source dynamic island client for desktop. Built with Tauri, React, and TypeScript, bringing the utility and aesthetic of the dynamic island to macOS and Windows."
# Topics as an array for proper handling
TOPICS=(tauri react desktop-app typescript dynamic-island macos windows cross-platform plugin-system rust motion)

echo "📝 Updating repository metadata for $REPO"
echo ""

# Check if gh is installed
if ! command -v gh &> /dev/null; then
    echo "❌ GitHub CLI (gh) is not installed."
    echo "Please install it from: https://cli.github.com/"
    exit 1
fi

# Check if authenticated
if ! gh auth status &> /dev/null; then
    echo "❌ Not authenticated with GitHub CLI."
    echo "Please run: gh auth login"
    exit 1
fi

echo "✅ GitHub CLI is authenticated"
echo ""

# Update description
echo "Updating repository description..."
if gh repo edit "$REPO" --description "$DESCRIPTION"; then
    echo "✅ Description updated successfully"
else
    echo "❌ Failed to update description"
    exit 1
fi

echo ""

# Add topics
echo "Adding repository topics..."
# Build the topic arguments
TOPIC_ARGS=()
for topic in "${TOPICS[@]}"; do
    TOPIC_ARGS+=(--add-topic "$topic")
done

if gh repo edit "$REPO" "${TOPIC_ARGS[@]}"; then
    echo "✅ Topics added successfully"
else
    echo "❌ Failed to add topics"
    exit 1
fi

echo ""
echo "🎉 Repository metadata updated successfully!"
echo ""
echo "View the repository: https://github.com/$REPO"
