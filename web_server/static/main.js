var folders = [];

async function q(url, method, data) {
  var h = {
    "Accept": "application/json",
    "Content-Type": "application/json",
  };
  var token = localStorage.getItem("token");
  if(token != null) h["Authorization"] = "token "+token;
  var request = {method: method, headers: h, };
  if(data != null) request["body"] = JSON.stringify(data);
  const response = await fetch(url, request);
  return response;
}

function get_link(href, html, classes) {
  var a = document.createElement("a");
  a.setAttribute("href", href);
  if(classes != null) a.setAttribute("class", classes);
  a.innerHTML = html;
  return a;
}

function get_folder_dropdown(hash_id) {
  var e = document.createElement("div");
  e.setAttribute("class", "dropdown is-hoverable");
  var t = document.createElement("div");
  t.setAttribute("class", "dropdown-trigger");
  t.innerHTML = '<button class="button is-small"><span>...</span></button>';
  e.append(t);
  var m = document.createElement("div");
  m.setAttribute("class", "dropdown-menu");
  m.setAttribute("id", "ddd-"+hash_id);
  e.append(m);
  var c = document.createElement("div");
  c.setAttribute("class", "dropdown-content");
  c.append(get_link("javascript:read_folder('"+hash_id+"')", '<i class="mdi mdi-check-all"></i> Mark all as read', "dropdown-item"));
  c.append(get_link("javascript:share('"+hash_id+"')", '<i class="mdi mdi-share-variant"></i> Share', "dropdown-item"));
  c.append(get_link("javascript:filter_folder('"+hash_id+"')", '<i class="mdi mdi-filter-cog"></i> Filter', "dropdown-item"));
  c.append(get_link("javascript:modal_folder('"+hash_id+"')", '<i class="mdi mdi-form-textbox"></i> Rename', "dropdown-item"));
  var h = document.createElement("hr");
  h.setAttribute("class", "dropdown-divider");
  c.append(h);
  c.append(get_link("javascript:delete_folder('"+hash_id+"')", '<i class="mdi mdi-delete"></i> Delete', "dropdown-item"));
  m.append(c);
  return e;
}
function display_folders() {
  var el = document.getElementById("folders");
  var el2 = document.getElementById("feed_folder");
  el.textContent = ""; el2.textContent = "";
  for(const f of folders) {
    const id = f["hash_id"];
    var e = document.createElement("li");
    var link = document.createElement("a");
    link.setAttribute("href", "#");
    link.setAttribute("id", "f_"+id);
    link.innerHTML = '<i class="mdi mdi-folder"></i> ' + f.name;
    console.log("TODO FOLDERS");
    e.append(link);
    var right = document.createElement("span");
    right.setAttribute("class", "is-pullled-right");
    var badge = document.createElement("div");
    badge.textContent = "0";
    badge.setAttribute("id", "f_n_"+id);
    badge.setAttribute("class", "tag is-rounded");
    right.append(badge);
    right.append(get_folder_dropdown(id));
    e.append(right);
    el.append(e);

    var e2 = document.createElement("option");
    e2.setAttribute("value", id);
    e2.textContent = f.name;
    el2.append(e2);
  }
}

function get_folders() {
  q("folders/", "GET", null).then((response) => {
    if (response.status === 200) {
      response.json().then(l => {
        folders = l;
        display_folders();
      })
    }
  });
}

function update_ui(logged) {
  let els = Array.from(document.getElementsByClassName("l"));
  if (logged) {
    els.forEach((e) => {
      e.classList.remove("is-hidden");
    });
    document.getElementById("anonymous").classList.add("is-hidden");
    get_folders();
    q("feeds/", "GET", null).then((response) => {
      if (response.status === 200) {
        console.log("TODO FEEDS");
      }
    });
  } else {
    els.forEach((e) => {
      e.classList.add("is-hidden");
    });
    document.getElementById("anonymous").classList.remove("is-hidden");
  }
}
window.onload = function () {
  var token = localStorage.getItem("token");
  if (token != null) {
    update_ui(true);
    //renew token everytime you log in
    q("tokens/" + token, "PATCH", null).then((response) => {
      if (response.status === 200 ) {
        response.json().then((v) => { localStorage.setItem("token", v); });
      }
    });
    //TODO: replace login/register form with user menu, get folders, get feeds, ...
  }
};
document.addEventListener("DOMContentLoaded", () => {
  // Functions to open and close a modal
  function openModal($el) {
    $el.classList.add("is-active");
  }
  function closeModal($el) {
    $el.classList.remove("is-active");
  }

  function closeAllModals() {
    (document.querySelectorAll(".modal") || []).forEach(($modal) => {
      closeModal($modal);
    });
  }

  // Add a click event on buttons to open a specific modal
  (document.querySelectorAll(".js-modal-trigger") || []).forEach(($trigger) => {
    const modal = $trigger.dataset.target;
    const $target = document.getElementById(modal);

    $trigger.addEventListener("click", () => {
      openModal($target);
    });
  });

  // Add a click event on various child elements to close the parent modal
  (
    document.querySelectorAll(
      ".modal-background, .modal-close, .modal-card-head .delete, .modal-card-foot .cancel"
    ) || []
  ).forEach(($close) => {
    const $target = $close.closest(".modal");
    $close.addEventListener("click", () => {
      closeModal($target);
    });
  });

  // Add a keyboard event to close all modals
  document.addEventListener("keydown", (event) => {
    const e = event || window.event;
    if (e.key == "Escape") {
      closeAllModals();
    }
  });
});

function logout() {
  localStorage.clear();
  update_ui(false);
}

function login() {
  let u = document.getElementById("lu").value;
  let p = document.getElementById("lp").value;
  q("login", "POST", { username: u, clear_password: p }).then(
    (response) => {
      if (response.status === 200) {
        document.getElementById("wrong_credentials").classList.add("is-hidden");
        response.json().then((v) => {
          localStorage.setItem("token", v);
          update_ui(true);
        });
        //TODO: get folders and feeds list, get articles
      } else {
        document.getElementById("wrong_credentials").classList.remove("is-hidden");
      }
    },
    function (err) {}
  );
}
function register() {
  let u = document.getElementById("ru");
  let p = document.getElementById("rp");
  let p2 = document.getElementById("rpc");
  if(p.value!=p2.value) {
    document.getElementById("rpc_diff").classList.remove("is-hidden");
  }
  q("account", "POST", {
    "username": u.value,
    "clear_password": p.value,
    "clear_password_2": p2.value,
  }).then(
    (response) => {
      if (response.status === 201) {
        document.getElementById("ru_exists").classList.add("is-hidden");
        document.getElementById("rp_weak").classList.add("is-hidden");
        document.getElementById("rpc_diff").classList.add("is-hidden");
        u.value = "";
        p.value = "";
        p2.value = "";
        //TODO: modal OK and ask to log in
      } else {
        if (response.status === 400) {
          response.json().then((msg) => {
            if (msg == "USERNAME_ALREADY_EXISTS") {
              document.getElementById("ru_exists").classList.remove("is-hidden");
            } else if ((msg == "PASSWORD_TOO_WEEK") || msg.startsWith("PASSWORD_STRENGTH:")) {
              document.getElementById("rp_weak").classList.remove("is-hidden");
            } else if (msg == "DIFFERENT_PASSWORDS") {
              document.getElementById("rpc_diff").classList.remove("is-hidden");
            }
          });
        }
      }
    },
    function (err) {}
  );
}

let folder_input = document.getElementById("folder");
let folder_hid = document.getElementById("folder_hid");
let folder_msg_length = document.getElementById("md-length");
let folder_msg_exists = document.getElementById("md-exists");
let folder_modal = document.getElementById("md");
let folder_title_add = document.getElementById("mdta");
let folder_title_edit = document.getElementById("mdte");

function modal_folder(hid) {
  folder_input.value = "";
  if(hid == undefined) {
    folder_hid.value = "";
    folder_title_add.classList.remove("is-hidden");
    folder_title_edit.classList.add("is-hidden");
  } else {
    folder_hid.value = hid;
    folder_input.value = document.getElementById("f_"+hid).textContent.trim();
    folder_title_add.classList.add("is-hidden");
    folder_title_edit.classList.remove("is-hidden");
  }
  folder_modal.classList.add("is-active");
}

function save_folder() {
  folder_msg_length.classList.add("is-hidden");
  folder_msg_exists.classList.add("is-hidden");
  if(folder_input.value.length < 3 || folder_input.value.length > 64) {
    folder_msg_length.classList.remove("is-hidden");
  } else {
    if(folder_hid.value == "") q("folders/", "POST", folder_input.value).then((response) => {
      if (response.status === 201) {
        folders.push({"hash_id": response.text(), "name": folder_input.value});
        folders.sort(sort_by_name);
        display_folders();
        folder_modal.classList.remove("is-active");
        folder_input.value = "";
      } else {
        folder_msg_exists.classList.remove("is-hidden");
      }
    });
    else q("folders/"+folder_hid.value, "PATCH", folder_input.value).then((response) => {
      if (response.status === 204) {
        for(var i=0; i<folders.length; i++) {
          if(folders[i]["hash_id"] == folder_hid.value) {
            folders[i]["name"] = folder_input.value;
            folders.sort(sort_by_name);
            break;
          }
        }
        display_folders();
        folder_modal.classList.remove("is-active");
        folder_hid.value = "";
        folder_input.value = "";
      } else {
        folder_msg_exists.classList.remove("is-hidden");
      }
    });
  }
}
function delete_folder(hash_id) {
  q("folders/"+hash_id, "DELETE", null).then((response) => {
    if (response.status === 204) {
      var e = document.getElementById("f_"+hash_id);
      var b = [];
      for(const f of folders) {
        if(f["hash_id"] != hash_id) b.push(f);
      }
      e.parentElement.remove();
    }
    //TODO: else {handle error}
  });
}

let feed_modal = document.getElementById("mr");
let feed_hid = document.getElementById("feed_hid");
let feed_url = document.getElementById("feed_url");
let feed_name = document.getElementById("feed_name");
let feed_xpath = document.getElementById("feed_xpath");
let feed_folder = document.getElementById("feed_folder"); // folder hash ID
let feed_inject = document.getElementById("feed_inject");
let feed_msg_url = document.getElementById("mr-url");
let feed_msg_folder = document.getElementById("mr-folder");
let feed_msg_xpath = document.getElementById("mr-xpath");
let feed_msg_exists = document.getElementById("mr-exists");
let feed_msg_checking = document.getElementById("mr-links");
let feed_title_add = document.getElementById("mrta");
let feed_title_edit = document.getElementById("mrte");

function is_valid_http_url(string) {
  let url;
  try {
    url = new URL(string);
  } catch (_) {
    return false;
  }
  return url.protocol === "http:" || url.protocol === "https:";
}

function modal_feed(hid) {
  if(hid == undefined) {
    feed_hid.value = "";
    feed_title_add.classList.remove("is-hidden");
    feed_title_edit.classList.add("is-hidden");
  } else {
    feed_hid.value = hid;
    feed_input.value = document.getElementById("r_"+hid).textContent.trim();
    feed_title_add.classList.add("is-hidden");
    feed_title_edit.classList.remove("is-hidden");
  }
  feed_modal.classList.add("is-active");
}

function save_feed() {
  feed_msg_url.classList.add("is-hidden");
  feed_msg_folder.classList.add("is-hidden");
  feed_msg_xpath.classList.add("is-hidden");
  feed_msg_exists.classList.add("is-hidden");
  feed_msg_checking.classList.add("is-hidden");
  if (!is_valid_http_url(feed_url.value)) {
    feed_msg_url.classList.remove("is-hidden");
    return;
  }
  if (feed_folder.value == "" || feed_folder.selectedIndex == -1) {
    feed_msg_folder.classList.remove("is-hidden");
    return;
  }
  var data = {"url": feed_url.value, "folder": feed_folder.value,
    "name": feed_name.value, "xpath": feed_xpath.value, "inject": feed_inject.checked
  };
  //TODO: POST/PATCH feed, handle error messages or empty fields if OK
  feed_msg_checking.classList.remove("is-hidden");
  if(feed_hid.value == "") q("feeds/", "POST", data).then((response) => {
    if (response.status === 201) {
      //folders.push({"hash_id": response.text(), "name": folder_input.value});
      display_feeds();
    } else {
      feed_msg_exists.classList.remove("is-hidden");
    }
  });
  else q("feeds/"+feed_hid.value, "PATCH", data).then((response) => {
    if (response.status === 204) {
      for(var i=0; i<folders.length; i++) {
        if(folders[i]["hash_id"] == folder_hid.value) {
          //folders[i]["name"] = folder_input.value;
          break;
        }
      }
      display_feeds();
    } else {
      feed_msg_exists.classList.remove("is-hidden");
      return;
    }
  });
  feed_modal.classList.remove("is-active");
  feed_hid.value = "";
  feed_url.value = "";
  feed_xpath.value = "";
  feed_folder.selectedIndex = -1;
  feed_name = "";
  feed_inject.checked = true;
}

function display_feeds() {
}

function sort_by_name(a, b) {
  let na = a.name.toLowerCase(), nb = b.name.toLowerCase();
  if(na < nb) return -1;
  if(na > nb) return 1;
  return 0;
}
