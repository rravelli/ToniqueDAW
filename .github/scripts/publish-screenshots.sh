#!/usr/bin/env bash
# Publish the images a Screenshots run left in DIR (its artifact) to the
# screenshots branch, and on a pull request, show what changed in a
# comment on it.
#
# The artifact comes from a run of the pull request's code: nothing in it
# is trusted. Only PNGs with plain names are taken, the pull request is
# found from the run's commit, not the artifact, and file names reach the
# comment only once checked.
#
# Environment: REPO (owner/name), EVENT, HEAD_SHA and HEAD_BRANCH (of the
# Screenshots run), DEPLOY_KEY (writes the branch), GH_TOKEN (reads pull
# requests, comments).
#
#     .github/scripts/publish-screenshots.sh artifact
set -euo pipefail

dir=$1
branch=screenshots
marker='<!-- screenshots -->'
max_bytes=$((5 * 1024 * 1024))

# What's taken from the artifact: PNGs named like the README's images.
valid_png() {
    local path=$1
    [[ $(basename "$path") =~ ^[a-z0-9][a-z0-9-]{0,63}\.png$ ]] &&
        [ -f "$path" ] && [ ! -L "$path" ] &&
        [ "$(stat -c %s "$path")" -le "$max_bytes" ] &&
        [ "$(head -c 8 "$path" | od -An -tx1 | tr -d ' \n')" = 89504e470d0a1a0a ]
}

if [[ ! $HEAD_SHA =~ ^[0-9a-f]{40}$ ]]; then
    echo "Not a commit: $HEAD_SHA" >&2
    exit 1
fi

images=()
for path in "$dir"/*.png; do
    if valid_png "$path"; then
        images+=("$(basename "$path")")
    else
        echo "Skipping $(basename "$path"): not a plain PNG"
    fi
done
if [ ${#images[@]} -eq 0 ]; then
    echo "No images to publish."
    exit 0
fi

case $EVENT in
    push)
        if [ "$HEAD_BRANCH" != master ]; then
            echo "Nothing to publish for a push to another branch."
            exit 0
        fi
        target=master
        ;;
    pull_request)
        # The open pull request whose head is the rendered commit.
        pr=$(gh api --paginate "repos/$REPO/pulls?state=open&per_page=100" \
            --jq ".[] | select(.head.sha == \"$HEAD_SHA\") | .number" | head -n1)
        if [ -z "$pr" ]; then
            echo "No open pull request at $HEAD_SHA: nothing to publish."
            exit 0
        fi
        short=${HEAD_SHA:0:7}
        target=pr/$pr/$short
        ;;
    *)
        echo "Nothing to publish for a $EVENT run."
        exit 0
        ;;
esac

# The branch, over SSH with the deploy key: the only writer the ruleset
# lets through.
work=$(mktemp -d)
key=$(mktemp)
trap 'rm -rf "$work" "$key"' EXIT
printf '%s\n' "$DEPLOY_KEY" > "$key"
export GIT_SSH_COMMAND="ssh -i $key -o IdentitiesOnly=yes -o StrictHostKeyChecking=yes"
git clone --quiet --depth 1 --branch "$branch" "git@github.com:$REPO.git" "$work"

# A pull request keeps only its latest images.
if [ "$EVENT" = pull_request ]; then
    rm -rf "$work/pr/$pr"
fi
rm -rf "${work:?}/$target"
mkdir -p "$work/$target"
for name in "${images[@]}"; do
    cp "$dir/$name" "$work/$target/$name"
done

# The "before" of each changed image: master's, as the run compared it.
changed=()
if [ "$EVENT" = pull_request ]; then
    mkdir -p "$work/$target/before"
    while IFS= read -r name; do
        if [[ " ${images[*]} " == *" $name "* ]]; then
            changed+=("$name")
            if valid_png "$dir/before/$name"; then
                cp "$dir/before/$name" "$work/$target/before/$name"
            fi
        fi
    done < <(head -n 100 "$dir/changed.txt" 2>/dev/null || true)
fi

# Pull requests closed since: their images go.
open=$(gh api --paginate "repos/$REPO/pulls?state=open&per_page=100" --jq '.[].number')
for path in "$work"/pr/*/; do
    [ -d "$path" ] || continue
    number=$(basename "$path")
    if ! grep -qx "$number" <<<"$open"; then
        echo "Removing the images of closed pull request #$number"
        rm -rf "$path"
    fi
done

# One commit, replacing the last: the branch never grows a history.
cd "$work"
git checkout --quiet --orphan publish
git add --all
git -c user.name="github-actions[bot]" \
    -c user.email="41898282+github-actions[bot]@users.noreply.github.com" \
    commit --quiet -m "screenshots: ${target}"
git push --quiet --force origin "publish:$branch"
cd - > /dev/null
echo "Published to $branch/$target"

[ "$EVENT" = pull_request ] || exit 0

# One comment per pull request, updated on each push.
raw="https://raw.githubusercontent.com/$REPO/$branch/$target"
body=$(mktemp)
{
    echo "$marker"
    echo "### Screenshots"
    echo
    if [ ${#changed[@]} -eq 0 ]; then
        echo "No visual changes from master as of ${short}."
    else
        echo "Changed from master as of ${short}:"
        for name in "${changed[@]}"; do
            echo
            echo "**${name}**"
            echo
            if [ -f "$work/$target/before/$name" ]; then
                echo "| master | this pull request |"
                echo "| --- | --- |"
                echo "| ![master]($raw/before/$name) | ![this pull request]($raw/$name) |"
            else
                echo "New: ![this pull request]($raw/$name)"
            fi
        done
    fi
} > "$body"

comment=$(gh api --paginate "repos/$REPO/issues/$pr/comments?per_page=100" \
    --jq ".[] | select(.user.login == \"github-actions[bot]\" and (.body | startswith(\"$marker\"))) | .id" |
    head -n1)
if [ -n "$comment" ]; then
    gh api -X PATCH "repos/$REPO/issues/comments/$comment" -F body=@"$body" > /dev/null
    echo "Updated the comment on #$pr"
elif [ ${#changed[@]} -gt 0 ]; then
    gh api -X POST "repos/$REPO/issues/$pr/comments" -F body=@"$body" > /dev/null
    echo "Commented on #$pr"
fi
