# URLs

List URLs/endpoints.

## Web pages

* GET `/` display the home page

## Assets

* GET `/s` Frust assets
* GET `/a` article assets
* GET `/f` feed assets (icons)

## Account (user) management

* POST `/users/` register a new user and returns its UUID or an error message
* POST `/users/login` log the user and returns its UUID or an error message
* PATCH `/<user UUID>/` upgrade user's information (password, config, email, ...)
* DELETE `/<user UUID>/` delete the user (you must provide the password)
* POST `/<user UUID>/generate-new-token` generate a new UUID in case the user leak it
* POST `/<user UUID>/restore/` import OPML file or backup file (with saved feeds, filter rules, ...)
* GET `/<user UUID>/backup/` export to OPML or backup file

## Content

### Folder management

* GET `/<user UUID>/folders/` list all folders
* PATCH `/<user UUID>/folders/<folder id>/` allow user to rename the folder
* DELETE `/<user UUID>/folders/<folder id>/` delete the folder, a parameter can be added to delete feeds in this folder
* GET `/<user UUID>/folders/<folder id>/` list user feeds in this folder

### Feed (ATOM or RSS) management

* GET `/<user UUID>/feeds/` list all user feeds, possible parameters: search text, show only saved
* POST `/<user UUID>/feeds/` Add a new feed
* PATCH `/<user UUID>/feeds/` Edit feed URL, Xpath, folder, ...
* DELETE `/<user UUID>/feeds/` delete a feed
* GET `/<user UUID>/feeds/<feed id>/` show only this feed full contents
* GET `/<user UUID>/feeds/<feed id>/<article id>/export` export to HTML, markdown, PDF

### Article management

* GET `/<user UUID>/feeds/<feed id>/<article id>/` load article content and extra details specific to the feed format
* PATCH `/<user UUID>/feeds/<feed id>/<article id>/` mark as read/unread, save the current article by setting the flag to keep it

### Filter management

* GET `/<user UUID>/filters/` list all filters with associated feed URLs
* POST `/<user UUID>/filters/` create a new filter
* PATH `/<user UUID>/filters/<filter ID>/` edit a filter
* DELETE `/<user UUID>/filters/<filter ID>/` delete a filter
