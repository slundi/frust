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

function update_ui(logged) {
  let els = Array.from(document.getElementsByClassName("l"));
  if (logged) {
    els.forEach((e) => {
      e.classList.remove("is-hidden");
    });
    document.getElementById("anonymous").classList.add("is-hidden");
    q("folders/", "GET", null).then((response) => {
      if (response.status === 200) {
        let folders = document.getElementById("folders");
        response.json().then(a => {
          for(const f of a) {
            var e = document.createElement("li");
            var link = document.createElement("a");
            link.setAttribute("href", "#");
            link.setAttribute("id", "f_"+f.hash_id);
            link.innerHTML = f.name;
            console.log("TODO FOLDERS");
            e.append(link);
            folders.append(e);
          }
        });
      }
    });
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
      ".modal-background, .modal-close, .modal-card-head .delete, .modal-card-foot .button"
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
        update_ui(true);
        response.json().then((v) => {
          localStorage.setItem("token", v);
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
