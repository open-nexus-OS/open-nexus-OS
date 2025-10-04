# Nexus IDL Schemas

This directory hosts the Cap'n Proto schemas used for Nexus control-plane messaging.
Large payloads such as application bundles travel via VMOs and are referenced here by
handle identifiers; Cap'n Proto only carries the metadata required to negotiate
those transfers.
