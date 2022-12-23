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
  c.append(get_link("javascript:rename_folder('"+hash_id+"')", '<i class="mdi mdi-form-textbox"></i> Rename', "dropdown-item"));
  var h = document.createElement("hr");
  h.setAttribute("class", "dropdown-divider");
  c.append(h);
  c.append(get_link("javascript:delete_folder('"+hash_id+"')", '<i class="mdi mdi-delete"></i> Delete', "dropdown-item"));
  m.append(c);
  return e;
}
function display_folders() {
  const a = JSON.parse(localStorage.getItem("folders"));
  var folders = document.getElementById("folders");
  folders.textContent = "";
  for(const f of a) {
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
    folders.append(e);
  }
}

function get_folders() {
  q("folders/", "GET", null).then((response) => {
    if (response.status === 200) {
      let folders = document.getElementById("folders");
      response.text().then(t => {
        localStorage.setItem("folders", t);
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
            } else if (msg == "PASSWORD_TOO_WEEK") {
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

function add_folder() {
  let e = document.getElementById("folder");
  let e_length = document.getElementById("md-length");
  let e_exists = document.getElementById("md-exists");
  e_length.classList.add("is-hidden");
  e_exists.classList.add("is-hidden");
  if(e.value.length < 3 || e.value.length > 64) {
    e_length.classList.remove("is-hidden");
  } else {
    q("folders/", "POST", e.value).then((response) => {
      if (response.status === 201) {
        let f = JSON.parse(localStorage.getItem("folders"));
        f.push({"hash_id": response.text(), "name": e.value});
        f.sort(sort_by_name);
        localStorage.setItem("folders", JSON.stringify(f));
        display_folders();
        document.getElementById("md").classList.remove("is-active");
        e.value = "";
      } else {
        e_exists.classList.remove("is-hidden");
      }
    });
  }
}
function delete_folder(hash_id) {
  q("folders/"+hash_id, "DELETE", null).then((response) => {
    if (response.status === 204) {
      var e = document.getElementById("f_"+hash_id);
      var a = JSON.parse(localStorage.getItem("folders"));
      var b = [];
      for(const f of a) {
        if(f["hash_id"] != hash_id) b.push(f);
      }
      localStorage.setItem("folders", JSON.stringify(b));
      e.parentElement.remove();
    }
    //TODO: else {handle error}
  });
}

function sort_by_name(a, b) {
  let na = a.name.toLowerCase(), nb = b.name.toLowerCase();
  if(na < nb) return -1;
  if(na > nb) return 1;
  return 0;
}
