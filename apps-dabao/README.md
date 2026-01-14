# Apps for Dabao

These are applications targeting a minimal set of Xous services for hardware configurations that are basically "just the chip".

## Building with Docker
From the root of the repository, run:

```shell
mkdir -p target && docker build --file apps-dabao/Dockerfile --build-arg app=helloworld --output target .
```

The `uf2` files will be under `target/`.
