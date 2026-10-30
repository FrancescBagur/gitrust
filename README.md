# gitrust

Cliente Git de escritorio en Rust. El objetivo es cubrir el flujo diario sin
depender de una licencia comercial, empezando por lo que más cuesta encontrar
gratis y bien hecho: **el grafo de commits con todas las ramas entrelazadas**.

## Estado

- [x] Abrir un repositorio y recorrer todas las referencias
- [x] Trazado del grafo en carriles, con fusiones y colores por rama
- [x] Pintado virtualizado (solo las filas visibles)
- [x] Lista de cambios separada entre índice y árbol de trabajo
- [x] Diff por fichero, stage y unstage, y commit con título y descripción
- [ ] Stage y unstage por hunk
- [ ] Enmendar el último commit
- [ ] Cambio de rama, push, pull
- [ ] Merge con editor de conflictos, rebase interactivo, stash, worktrees

## Arquitectura

```
crates/
  gitrust-core/     lógica Git, sin dependencias de UI
    repo.rs         repositorio, commits, índice y diffs (libgit2)
    graph.rs        algoritmo de carriles
    status.rs       estado del árbol de trabajo
    diff.rs         diff troceado en hunks
    message.rs      composición del mensaje de commit
    model.rs        tipos compartidos
  gitrust-ui/       aplicación egui
    main.rs         ventana, estado y ejecución de acciones
    graph_view.rs   pintado del grafo
    changes_view.rs listas de cambios, diff y cuadro de commit
```

Las vistas no tocan el repositorio: devuelven una `Action` y `main.rs` la
ejecuta. Eso mantiene el pintado libre de efectos secundarios.

El núcleo no conoce la UI. Si en algún momento egui deja de encajar, se
sustituye `gitrust-ui` y la lógica Git sigue intacta.

**Backend Git:** `git2` (libgit2) para el grueso de las operaciones. Para
credential helpers, LFS y hooks el plan es delegar en el binario `git` real,
igual que hacen los clientes comerciales, porque libgit2 no los cubre bien.

## Compilar

Dependencias del sistema en Debian/Ubuntu/Pop!_OS:

```bash
sudo apt install -y rustc cargo cmake libssl-dev libxkbcommon-dev \
  libwayland-dev libx11-dev libxcursor-dev libxi-dev libxrandr-dev \
  libgl1-mesa-dev
```

Luego:

```bash
cargo run --release -- /ruta/a/un/repositorio
```

Sin argumento abre el repositorio del directorio actual.

## Tests

```bash
cargo test -p gitrust-core
```

El algoritmo del grafo es la pieza con más riesgo de regresión, y las
operaciones sobre el índice son las que escriben en tus ficheros. Son las dos
que llevan cobertura: doce pruebas, seis de ellas contra repositorios reales
creados en `/tmp` y borrados al terminar.

## Licencia

GPL-3.0-only. El texto completo está en [LICENSE](LICENSE).

libgit2 es GPLv2 con excepción de enlazado y los crates de Rust son
`MIT OR Apache-2.0`, así que ninguna dependencia obligaba a esta elección: una
herramienta hecha para dejar de pagar una licencia no debería poder cerrarla
nadie más.
