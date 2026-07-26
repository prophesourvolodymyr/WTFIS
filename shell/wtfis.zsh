cd() {
  local cd_status previous_dir="$PWD"
  if builtin cd "$@"; then
    export WTFIS_PREV_CD="$previous_dir"
    unset WTFIS_LAST_CD
    return 0
  fi
  cd_status=$?
  if [ "$#" -eq 1 ] && [ "$1" != "-" ]; then
    export WTFIS_LAST_CD="$1"
    printf 'Try: wtfis --up\n' >&2
  fi
  return "$cd_status"
}

wtfis() {
  local output exit_code selected selected_path selected_command command_status previous_cd previous_dir where_only
  where_only=false
  if [ "$#" -eq 1 ]; then
    case "$1" in
      --home|--prev)
        previous_dir="$PWD"
        if [ "$1" = "--home" ]; then
          export WTFIS_PREV_CD="$previous_dir"
          builtin cd -- "${HOME:-$PWD}"
        else
          if [ -z "${WTFIS_PREV_CD:-}" ]; then
            printf 'wtfis: no previous directory is available\n' >&2
            return 1
          fi
          builtin cd -- "$WTFIS_PREV_CD"
        fi
        exit_code=$?
        if [ "$exit_code" -ne 0 ]; then return "$exit_code"; fi
        if [ "$1" = "--prev" ]; then export WTFIS_PREV_CD="$previous_dir"; fi
        unset WTFIS_LAST_CD
        return 0
        ;;
      --where) where_only=true ;;
    esac
  fi
  if [ "$#" -eq 1 ] && [ "$1" = "--up" ] && [ -z "${WTFIS_LAST_CD:-}" ]; then
    previous_cd="$(fc -ln -2 2>/dev/null)"
    previous_cd="${previous_cd%%$'\n'*}"
    while [[ "$previous_cd" == [[:space:]]* ]]; do previous_cd="${previous_cd#?}"; done
    case "$previous_cd" in
      cd\ *) export WTFIS_LAST_CD="${previous_cd#cd }" ;;
    esac
  fi
  output="$(mktemp "${TMPDIR:-/tmp}/wtfis.XXXXXX")" || return
  WTFIS_OUTPUT="$output" command wtfis "$@" >/dev/tty 2>/dev/tty
  exit_code=$?
  command_status=$exit_code
  if [ "$exit_code" -eq 0 ] && [ -s "$output" ]; then
    selected="$(<"$output")"
    selected_path="${selected%%$'\n'*}"
    selected_command="${selected#*$'\n'}"
    if [ "$selected_command" = "$selected" ]; then
      selected_command=""
    fi
    if [ "$where_only" = true ]; then
      printf '%s\n' "$selected_path"
    elif [ -n "$selected_path" ]; then
      previous_dir="$PWD"
      if builtin cd -- "$selected_path"; then
        export WTFIS_PREV_CD="$previous_dir"
        unset WTFIS_LAST_CD
        if [ -n "$selected_command" ]; then
          eval "$selected_command"
          command_status=$?
        fi
      fi
    fi
  fi
  rm -f "$output"
  return "$command_status"
}

cdd() { wtfis "$@"; }
