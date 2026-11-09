export const routes = {
  home: "/",
  login: "/login",
};

export function render(path: string): string {
  if (path === routes.login) return "login form";
  return "home";
}
