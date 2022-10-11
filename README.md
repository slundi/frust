# Frust

A lightweight RSS feeder

Lecteur de flux RSS / aggrégateur RSS avec fonctionnalités sympas

Lecteur de flux RSS avec une interface similaire à Feedly.

### Fonctionalités

* Scroll spy pour marker les élements comme lu (comme Feedly)
* Import/export OPML file
* Search on all feeds (à lire, sauvegardé)
* Set filter to remove irrelevant feeds
* Save feed
* Categories
* Xpath option to get article content and not brief

## Libraries

* [feed-rs](https://crates.io/crates/feed-rs)
* [opml](https://crates.io/crates/opml)
* actix

## Endpoints

* GET `/` display the home page
* POST `/users/` register a new user and returns its UUID or an error message
* POST `/users/login` log the user and returns its UUID or an error message
* POST `/<user UUID>/generate-new-token` generate a new UUID in case the user leak it
* POST `/<user UUID>/restore/` import OPML file or backup file (with saved feeds, filter rules, ...)
* GET `/<user UUID>/backup/` export to OPML or backup file

* GET `/<user UUID>/feeds/` list all user feeds, possible parameters: search text, show only saved
* POST `/<user UUID>/feeds/` Add a new feed
* PATCH `/<user UUID>/feeds/` Edit feed URL, Xpath, folder, ...
* DELETE `/<user UUID>/feeds/` delete a feed
* GET `/<user UUID>/feeds/<feed id>/` show only this feed full contents
* GET `/<user UUID>/feeds/<feed id>/<article id>/export` export to HTML, markdown, PDF
* PATCH `/<user UUID>/feeds/<feed id>/<article id>/` mark as read/unread, save the current article by setting the flag to keep it

* GET `/<user UUID>/folders/` list all folders
* PATCH `/<user UUID>/folders/<folder id>/` allow user to rename the folder
* DELETE `/<user UUID>/folders/<folder id>/` delete the folder, a parameter can be added to delete feeds in this folder
* GET `/<user UUID>/folders/<folder id>/` list user feeds in this folder

* GET `/<user UUID>/filters/` list all filters with associated feed URLs
* POST `/<user UUID>/filters/` create a new filter
* PATH `/<user UUID>/filters/<filter ID>/` edit a filter
* DELETE `/<user UUID>/filters/<filter ID>/` delete a filter
