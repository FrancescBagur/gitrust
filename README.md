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
- [x] Stage y unstage por hunk suelto
- [x] Tres paneles: ramas, grafo y detalle del commit
- [x] Tema propio: una sola familia de color saturado, la de los carriles
- [x] Los cambios sin confirmar, como una fila más arriba del grafo
- [x] Barra de acciones: pull, push, rama nueva y stash
- [x] Columna de ramas y etiquetas a la izquierda del grafo
- [x] Cambiar de rama, también a una que solo existe en el remoto
- [x] Enmendar el último commit, con aviso si ya salió al remoto
- [x] Etiquetas: crear ligeras o anotadas, y borrarlas
- [ ] Merge con editor de conflictos, rebase interactivo, worktrees

## Arquitectura

```
crates/
  gitrust-core/     lógica Git, sin dependencias de UI
    repo.rs         repositorio, commits, índice y diffs (libgit2)
    branch.rs       crear ramas y cambiar de una a otra
    tag.rs          etiquetas ligeras y anotadas
    remote.rs       traer, llevar, y la distancia con la rama remota
    stash.rs        la pila de cambios apartados
    graph.rs        algoritmo de carriles, y la fila de cambios sin confirmar
    status.rs       estado del árbol de trabajo
    diff.rs         diff troceado en hunks
    detail.rs       detalle de un commit y referencias
    message.rs      composición del mensaje de commit
    model.rs        tipos compartidos, y qué referencia manda en un commit
  gitrust-ui/       aplicación egui
    main.rs         ventana, estado y ejecución de acciones
    theme.rs        color, tipografía y espaciado
    action_bar.rs   pull, push, rama y stash
    refs_view.rs    barra lateral de ramas y etiquetas
    graph_view.rs   pintado del grafo y de la columna de referencias
    detail_view.rs  panel de detalle del commit
    changes_view.rs listas de cambios, diff y cuadro de commit
    diff_view.rs    pintado compartido de líneas de diff
```

Las vistas no tocan el repositorio: devuelven una `Action` y `main.rs` la
ejecuta. Eso mantiene el pintado libre de efectos secundarios.

El núcleo no conoce la UI. Si en algún momento egui deja de encajar, se
sustituye `gitrust-ui` y la lógica Git sigue intacta.

Lo que sale a la red —traer y llevar— se va a un hilo suyo, que abre su propio
repositorio y devuelve una frase por un canal. El hilo que pinta no espera a
nadie.

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
operaciones que escriben —índice, stash, ramas y remoto— son las que tocan tus
ficheros. Son las que llevan cobertura: sesenta y una pruebas, cuarenta y una de
ellas contra repositorios reales creados en `/tmp` y borrados al terminar.

Las del remoto hablan con un repositorio bare en otro directorio temporal. Por
una ruta de disco se recorre el mismo camino que con un remoto de verdad —los
refspecs, el avance rápido, la rama que pasa a seguir a otra— sin depender de
que haya red ni credenciales.

## Licencia

GPL-3.0-only. El texto completo está en [LICENSE](LICENSE).

libgit2 es GPLv2 con excepción de enlazado y los crates de Rust son
`MIT OR Apache-2.0`, así que ninguna dependencia obligaba a esta elección: una
herramienta hecha para dejar de pagar una licencia no debería poder cerrarla
nadie más.
