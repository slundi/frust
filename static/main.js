function update_ui(logged) {
  let els = Array.from(document.getElementsByClassName("l"));
  if (logged) {
    els.forEach((e) => {
      e.classList.remove("is-hidden");
    });
    document.getElementById("anonymous").classList.add("is-hidden");
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
  let u = document.getElementById("ru").value;
  let p = document.getElementById("rp").value;
  let p2 = document.getElementById("rpc").value;
  q("account", "POST", {
    username: u,
    clear_password: p,
    clear_password_2: p2,
  }).then(
    (response) => {
      //TODO: modal OK and ask to log in
    },
    function (err) {}
  );
}

async function q(url, method, data) {
  const response = await fetch(url, {
    method: method,
    headers: {
      Accept: "application/json",
      "Content-Type": "application/json",
    },
    body: JSON.stringify(data),
  });
  return response;
}
