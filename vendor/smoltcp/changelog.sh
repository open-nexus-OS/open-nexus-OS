for pr in $(git log v0.9.1..main --grep 'Merge' | grep -oP '#[0-9]+' | sort -u); do 
    echo $(gh pr view $pr --json title -q .title) "($pr)"
done