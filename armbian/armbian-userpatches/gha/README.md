# gha matrix automation

## how does this work

TLDR; it's complicated I'll get back to you.

it's based on [rpardini's tooling](https://github.com/rpardini/armbian-build/tree/extensions/userpatches/gha)

as [noted here](https://github.com/rpardini/armbian-build/blob/f487f305753465a058082bf6ccada7588f5b1aac/userpatches/gha/chunks/750.single_repo.yaml#L198-L205) a pair of secrets for GPG signing key was configured in GHA

## updating matrix

### light changes

if just updating board list, this should be sufficient

update `armbian-userpatches/targets.yaml`

### broader changes.. 

anything beyond a board item, you need ot regerate everything

assumes armbian-build is already checked out

run this from the `airwaves-os/armbian` path

```
rclone sync armbian-userpatches armbian-build/userpatches;pushd armbian-build/;./compile.sh gha-matrix;./compile.sh gha-template MATRIX_ARTIFACT_CHUNKS=1 MATRIX_IMAGE_CHUNKS=1;cp output/info/artifact-image-complete-matrix.yml ../../.github/workflows/artifact-image-complete-matrix.yml ;popd
```
