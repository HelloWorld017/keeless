declare global {
  module '*?asset' {
    const url: string;
    export default url;
  }
}

export {};
