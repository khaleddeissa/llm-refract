# Event vocabulary

`generation`, `tool.call`, `retrieval`, `decision`, `state.change`, `checkpoint`,
`handoff`, `human`, `artifact`, `error`.

Provider/model fields belong in `attributes`. State events use `input` for the before state
and `output` for the after state. All events default to `RECORDED` replay policy.
