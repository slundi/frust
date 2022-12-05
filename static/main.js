window.onload = function () {
  var token = localStorage.getItem("token");
  if (token != null) {
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

function login() {
  let u = document.getElementById("lu").value;
  let p = document.getElementById("lp").value;
  console.log(q("login", "POST", {"username":u, "clear_password":p}));
}
function register() {
  let u = document.getElementById("ru").value;
  let p = document.getElementById("rp").value;
  let p2 = document.getElementById("rpc").value;
  console.log(q("register", "POST", {"username":u, "clear_password":p, "clear_password_2":p2}));
}

async function q(url, method, data) {
    try {
        const response = await fetch(url, {
            method: method,
            headers: {
                "Accept": "application/json",
                "Content-Type": "application/json",
            },
            body: JSON.stringify(data),
        });
        const res = response.json;
        if (res.status === 200) {
            console.log("Q OK");
        }
    } catch (err) {
        console.log("Q " + err);
    }
}
