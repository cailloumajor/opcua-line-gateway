uid := $(shell id -u)
gid := $(shell id -g)
modelcompiler := docker run --rm -u "$(uid):$(gid)" -v .:/schemas ghcr.io/opcfoundation/ua-modelcompiler
mcargs := -cg /dev/null -id 5000 -version v105 -suppress PredefinedNodes,Constants,JsonSchema,Classes,DataTypes

nodeset/%.NodeSet2.xml: modeldesign/%Model.xml
	$(modelcompiler) compile $(mcargs) -d2 /schemas/$< -o2 /schemas/$(@D)
